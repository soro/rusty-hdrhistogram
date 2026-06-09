use crate::concurrent::double_histogram::{ConcurrentDoubleHistogramWithPolicy, ConcurrentDoubleReadView, ConcurrentDoubleSnapshot};
use crate::concurrent::recordable_histogram::RecordableHistogram;
use crate::concurrent::recorder::{DoubleRecorderWithPolicy, Recorder, SingleWriterDoubleRecorderWithPolicy, SingleWriterRecorder};
use crate::concurrent::resizable_histogram::ResizableConcurrentHistogram;
use crate::concurrent::snapshot::{FixedSnapshot, ResizableSnapshot, Snapshot};
use crate::concurrent::static_histogram::FixedConcurrentHistogram;
use crate::concurrent::writer_reader_phaser::PhaseFlipGuard;
use crate::core::OverflowPolicy;
use crate::iteration::{
    DoubleAllValuesIterator, DoubleLinearIterator, DoubleLogarithmicIterator, DoublePercentileIterator, DoubleRecordedValuesIterator,
};
use crate::st::{DoubleHistogramWithPolicy, Histogram};
use std::mem;
use std::sync::atomic::{AtomicPtr, Ordering};

pub(crate) struct IntervalSampleCore<'a, 'b: 'a, T: 'a + RecordableHistogram> {
    parent_recorder: &'a Recorder<T>,
    histogram: AtomicPtr<T>,
    guard: PhaseFlipGuard<'b>,
}

impl<'a, 'b: 'a, T: RecordableHistogram> IntervalSampleCore<'a, 'b, T> {
    pub(in crate::concurrent) fn new(parent_recorder: &'a Recorder<T>, histogram: *mut T, guard: PhaseFlipGuard<'b>) -> Self {
        IntervalSampleCore {
            parent_recorder,
            histogram: AtomicPtr::new(histogram),
            guard,
        }
    }

    pub(crate) fn resample(self) -> Self {
        unsafe {
            let to_swap = self.histogram.load(Ordering::Acquire);
            // SAFETY: this sample owns the sampled-out histogram, and consuming
            // `self` prevents safe snapshot/read borrows from surviving resample.
            (*to_swap).clear_counts_for_reuse();
            let res = self.parent_recorder.perform_interval_sample(to_swap, &self.guard);
            self.histogram.store(res, Ordering::Release);
            self
        }
    }

    pub(crate) fn snapshot(&self) -> Snapshot<'_, T> {
        unsafe { Snapshot::new(&*self.histogram.load(Ordering::Acquire)) }
    }
}

/// An active sampled interval from a fixed concurrent recorder.
///
/// While this value is alive, it owns the recorder's exclusive sampling lease.
/// Writers continue recording into the next interval.
pub struct FixedIntervalSample<'a, 'b: 'a>(IntervalSampleCore<'a, 'b, FixedConcurrentHistogram>);

/// An active sampled interval from a resizable concurrent recorder.
///
/// While this value is alive, it owns the recorder's exclusive sampling lease.
/// Writers continue recording into the next interval.
pub struct ResizableIntervalSample<'a, 'b: 'a>(IntervalSampleCore<'a, 'b, ResizableConcurrentHistogram>);

macro_rules! impl_interval_sample_wrapper {
    ($sample:ident, $histogram:ty, $snapshot:ident) => {
        impl<'a, 'b: 'a> $sample<'a, 'b> {
            pub(in crate::concurrent) fn new(sample: IntervalSampleCore<'a, 'b, $histogram>) -> Self {
                $sample(sample)
            }

            /// Swap out and return the next sampled interval.
            ///
            /// This may wait for writer calls that were already in flight when
            /// the interval boundary was established.
            pub fn resample(self) -> Self {
                $sample(self.0.resample())
            }

            /// Return a read-only snapshot of the most recently sampled histogram.
            pub fn snapshot(&self) -> $snapshot<'_> {
                $snapshot::new(self.0.snapshot())
            }
        }
    };
}

impl_interval_sample_wrapper!(FixedIntervalSample, FixedConcurrentHistogram, FixedSnapshot);
impl_interval_sample_wrapper!(ResizableIntervalSample, ResizableConcurrentHistogram, ResizableSnapshot);

impl<'a, 'b: 'a, T: RecordableHistogram> Drop for IntervalSampleCore<'a, 'b, T> {
    fn drop(&mut self) {
        unsafe {
            self.guard.flip();
            mem::drop(Box::from_raw(self.histogram.load(Ordering::SeqCst)));
        }
    }
}

/// An active sampled interval from a concurrent double recorder.
///
/// While this value is alive, it owns the recorder's exclusive sampling lease.
/// Writers continue recording into the next interval.
pub struct DoubleIntervalSample<'a, 'b: 'a, P: OverflowPolicy> {
    parent_recorder: &'a DoubleRecorderWithPolicy<P>,
    histogram: AtomicPtr<ConcurrentDoubleHistogramWithPolicy<P>>,
    guard: PhaseFlipGuard<'b>,
}

impl<'a, 'b: 'a, P: OverflowPolicy> DoubleIntervalSample<'a, 'b, P> {
    pub(in crate::concurrent) fn new(
        parent_recorder: &'a DoubleRecorderWithPolicy<P>,
        histogram: *mut ConcurrentDoubleHistogramWithPolicy<P>,
        guard: PhaseFlipGuard<'b>,
    ) -> Self {
        DoubleIntervalSample {
            parent_recorder,
            histogram: AtomicPtr::new(histogram),
            guard,
        }
    }

    /// Swap out and return the next sampled interval.
    ///
    /// This may wait for writer calls that were already in flight when the
    /// interval boundary was established.
    pub fn resample(self) -> Self {
        unsafe {
            let to_swap = self.histogram.load(Ordering::Acquire);
            (*to_swap).reset();
            let res = self.parent_recorder.perform_interval_sample(to_swap, &self.guard);
            self.histogram.store(res, Ordering::Release);
            self
        }
    }

    fn raw_histogram(&self) -> &ConcurrentDoubleHistogramWithPolicy<P> {
        unsafe { &*self.histogram.load(Ordering::Acquire) }
    }

    pub fn snapshot(&self) -> ConcurrentDoubleSnapshot<'_, P> {
        ConcurrentDoubleSnapshot::new(self.raw_histogram())
    }

    pub fn percentiles(&self, percentile_ticks_per_half_distance: u32) -> DoublePercentileIterator<ConcurrentDoubleReadView<'_>> {
        self.raw_histogram().percentiles_snapshot(percentile_ticks_per_half_distance)
    }

    pub fn linear_bucket_values(&self, value_units_per_bucket: f64) -> DoubleLinearIterator<ConcurrentDoubleReadView<'_>> {
        self.raw_histogram().linear_bucket_values_snapshot(value_units_per_bucket)
    }

    pub fn logarithmic_bucket_values(
        &self,
        value_units_in_first_bucket: f64,
        log_base: f64,
    ) -> DoubleLogarithmicIterator<ConcurrentDoubleReadView<'_>> {
        self.raw_histogram()
            .logarithmic_bucket_values_snapshot(value_units_in_first_bucket, log_base)
    }

    pub fn all_values(&self) -> DoubleAllValuesIterator<ConcurrentDoubleReadView<'_>> {
        self.raw_histogram().all_values_snapshot()
    }

    pub fn recorded_values(&self) -> DoubleRecordedValuesIterator<ConcurrentDoubleReadView<'_>> {
        self.raw_histogram().recorded_values_snapshot()
    }
}

impl<'a, 'b: 'a, P: OverflowPolicy> Drop for DoubleIntervalSample<'a, 'b, P> {
    fn drop(&mut self) {
        unsafe {
            self.guard.flip();
            mem::drop(Box::from_raw(self.histogram.load(Ordering::SeqCst)));
        }
    }
}

/// An active sampled interval from a single-writer recorder.
///
/// While this value is alive, it owns the recorder's exclusive sampling lease.
/// The single writer continues recording into the next interval after the swap.
pub struct SingleWriterIntervalSample<'a, 'b: 'a> {
    parent_recorder: &'a SingleWriterRecorder,
    histogram: AtomicPtr<Histogram>,
    guard: PhaseFlipGuard<'b>,
}

impl<'a, 'b: 'a> SingleWriterIntervalSample<'a, 'b> {
    pub(in crate::concurrent) fn new(
        parent_recorder: &'a SingleWriterRecorder,
        histogram: *mut Histogram,
        guard: PhaseFlipGuard<'b>,
    ) -> Self {
        SingleWriterIntervalSample {
            parent_recorder,
            histogram: AtomicPtr::new(histogram),
            guard,
        }
    }

    /// Swap out and return the next sampled interval.
    ///
    /// This briefly waits for a writer call that is already in flight.
    pub fn resample(self) -> Self {
        unsafe {
            let to_swap = self.histogram.load(Ordering::Acquire);
            (*to_swap).reset();
            let res = self.parent_recorder.perform_interval_sample(to_swap, &self.guard);
            self.histogram.store(res, Ordering::Release);
            self
        }
    }

    fn raw_histogram(&self) -> &Histogram {
        unsafe { &*self.histogram.load(Ordering::Acquire) }
    }

    /// Return a read-only view of the most recently sampled histogram.
    pub fn snapshot(&self) -> &Histogram {
        self.raw_histogram()
    }
}

impl<'a, 'b: 'a> Drop for SingleWriterIntervalSample<'a, 'b> {
    fn drop(&mut self) {
        unsafe {
            self.guard.flip();
            mem::drop(Box::from_raw(self.histogram.load(Ordering::SeqCst)));
        }
    }
}

/// An active sampled interval from a single-writer double recorder.
///
/// While this value is alive, it owns the recorder's exclusive sampling lease.
/// The single writer continues recording into the next interval after the swap.
pub struct SingleWriterDoubleIntervalSample<'a, 'b: 'a, P: OverflowPolicy> {
    parent_recorder: &'a SingleWriterDoubleRecorderWithPolicy<P>,
    histogram: AtomicPtr<DoubleHistogramWithPolicy<P>>,
    guard: PhaseFlipGuard<'b>,
}

impl<'a, 'b: 'a, P: OverflowPolicy> SingleWriterDoubleIntervalSample<'a, 'b, P> {
    pub(in crate::concurrent) fn new(
        parent_recorder: &'a SingleWriterDoubleRecorderWithPolicy<P>,
        histogram: *mut DoubleHistogramWithPolicy<P>,
        guard: PhaseFlipGuard<'b>,
    ) -> Self {
        SingleWriterDoubleIntervalSample {
            parent_recorder,
            histogram: AtomicPtr::new(histogram),
            guard,
        }
    }

    /// Swap out and return the next sampled interval.
    ///
    /// This briefly waits for a writer call that is already in flight.
    pub fn resample(self) -> Self {
        unsafe {
            let to_swap = self.histogram.load(Ordering::Acquire);
            (*to_swap).reset();
            let res = self.parent_recorder.perform_interval_sample(to_swap, &self.guard);
            self.histogram.store(res, Ordering::Release);
            self
        }
    }

    fn raw_histogram(&self) -> &DoubleHistogramWithPolicy<P> {
        unsafe { &*self.histogram.load(Ordering::Acquire) }
    }

    /// Return a read-only view of the most recently sampled histogram.
    pub fn snapshot(&self) -> &DoubleHistogramWithPolicy<P> {
        self.raw_histogram()
    }
}

impl<'a, 'b: 'a, P: OverflowPolicy> Drop for SingleWriterDoubleIntervalSample<'a, 'b, P> {
    fn drop(&mut self) {
        unsafe {
            self.guard.flip();
            mem::drop(Box::from_raw(self.histogram.load(Ordering::SeqCst)));
        }
    }
}
