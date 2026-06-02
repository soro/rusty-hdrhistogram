use crate::concurrent::double_histogram::{ConcurrentDoubleHistogramImpl, ConcurrentDoubleReadView, ConcurrentDoubleSnapshot};
use crate::concurrent::recordable_histogram::RecordableHistogram;
use crate::concurrent::recorder::{DoubleRecorder, Recorder, SingleWriterDoubleRecorder, SingleWriterRecorder};
use crate::concurrent::resizable_histogram::ResizableConcurrentHistogram;
use crate::concurrent::snapshot::{ResizableSnapshot, Snapshot, StaticSnapshot};
use crate::concurrent::static_histogram::StaticHistogram;
use crate::concurrent::writer_reader_phaser::PhaseFlipGuard;
use crate::core::OverflowPolicy;
use crate::iteration::{
    DoubleAllValuesIterator, DoubleLinearIterator, DoubleLogarithmicIterator, DoublePercentileIterator, DoubleRecordedValuesIterator,
};
use crate::st::{DoubleHistogramImpl, Histogram};
use std::mem;
use std::sync::atomic::{AtomicPtr, Ordering};

pub(crate) struct LockingSample<'a, 'b: 'a, T: 'a + RecordableHistogram> {
    parent_recorder: &'a Recorder<T>,
    histogram: AtomicPtr<T>,
    guard: PhaseFlipGuard<'b>,
}

impl<'a, 'b: 'a, T: RecordableHistogram> LockingSample<'a, 'b, T> {
    pub(in crate::concurrent) fn new(parent_recorder: &'a Recorder<T>, histogram: *mut T, guard: PhaseFlipGuard<'b>) -> Self {
        LockingSample {
            parent_recorder,
            histogram: AtomicPtr::new(histogram),
            guard,
        }
    }

    pub(crate) fn resample(self) -> Self {
        unsafe {
            let to_swap = self.histogram.load(Ordering::Acquire);
            (*to_swap).clear_counts();
            let res = self.parent_recorder.perform_interval_sample(to_swap, &self.guard);
            self.histogram.store(res, Ordering::Release);
            self
        }
    }

    pub(crate) fn histogram(&self) -> Snapshot<'_, T> {
        unsafe { Snapshot::new(&*self.histogram.load(Ordering::Acquire)) }
    }
}

pub struct StaticLockingSample<'a, 'b: 'a>(LockingSample<'a, 'b, StaticHistogram>);

pub struct ResizableLockingSample<'a, 'b: 'a>(LockingSample<'a, 'b, ResizableConcurrentHistogram>);

macro_rules! impl_locking_sample_wrapper {
    ($sample:ident, $histogram:ty, $snapshot:ident) => {
        impl<'a, 'b: 'a> $sample<'a, 'b> {
            pub(in crate::concurrent) fn new(sample: LockingSample<'a, 'b, $histogram>) -> Self {
                $sample(sample)
            }

            pub fn resample(self) -> Self {
                $sample(self.0.resample())
            }

            pub fn histogram(&self) -> $snapshot<'_> {
                $snapshot::new(self.0.histogram())
            }

            /// Return a read-only snapshot of the most recently sampled histogram.
            ///
            /// This is an alias for [`histogram`](Self::histogram) with a name
            /// that better reflects the returned type.
            pub fn snapshot(&self) -> $snapshot<'_> {
                self.histogram()
            }
        }
    };
}

impl_locking_sample_wrapper!(StaticLockingSample, StaticHistogram, StaticSnapshot);
impl_locking_sample_wrapper!(ResizableLockingSample, ResizableConcurrentHistogram, ResizableSnapshot);

impl<'a, 'b: 'a, T: RecordableHistogram> Drop for LockingSample<'a, 'b, T> {
    fn drop(&mut self) {
        unsafe {
            self.guard.flip();
            mem::drop(Box::from_raw(self.histogram.load(Ordering::SeqCst)));
        }
    }
}

pub struct DoubleLockingSample<'a, 'b: 'a, P: OverflowPolicy> {
    parent_recorder: &'a DoubleRecorder<P>,
    histogram: AtomicPtr<ConcurrentDoubleHistogramImpl<P>>,
    guard: PhaseFlipGuard<'b>,
}

impl<'a, 'b: 'a, P: OverflowPolicy> DoubleLockingSample<'a, 'b, P> {
    pub(in crate::concurrent) fn new(
        parent_recorder: &'a DoubleRecorder<P>,
        histogram: *mut ConcurrentDoubleHistogramImpl<P>,
        guard: PhaseFlipGuard<'b>,
    ) -> Self {
        DoubleLockingSample {
            parent_recorder,
            histogram: AtomicPtr::new(histogram),
            guard,
        }
    }

    pub fn resample(self) -> Self {
        unsafe {
            let to_swap = self.histogram.load(Ordering::Acquire);
            (*to_swap).reset();
            let res = self.parent_recorder.perform_interval_sample(to_swap, &self.guard);
            self.histogram.store(res, Ordering::Release);
            self
        }
    }

    fn raw_histogram(&self) -> &ConcurrentDoubleHistogramImpl<P> {
        unsafe { &*self.histogram.load(Ordering::Acquire) }
    }

    pub fn histogram(&self) -> ConcurrentDoubleSnapshot<'_, P> {
        ConcurrentDoubleSnapshot::new(self.raw_histogram())
    }

    /// Return a read-only snapshot of the most recently sampled histogram.
    ///
    /// This is an alias for [`histogram`](Self::histogram) with the same naming
    /// convention as the integer recorder sample types.
    pub fn snapshot(&self) -> ConcurrentDoubleSnapshot<'_, P> {
        self.histogram()
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

impl<'a, 'b: 'a, P: OverflowPolicy> Drop for DoubleLockingSample<'a, 'b, P> {
    fn drop(&mut self) {
        unsafe {
            self.guard.flip();
            mem::drop(Box::from_raw(self.histogram.load(Ordering::SeqCst)));
        }
    }
}

pub struct SingleWriterLockingSample<'a, 'b: 'a> {
    parent_recorder: &'a SingleWriterRecorder,
    histogram: AtomicPtr<Histogram<u64>>,
    guard: PhaseFlipGuard<'b>,
}

impl<'a, 'b: 'a> SingleWriterLockingSample<'a, 'b> {
    pub(in crate::concurrent) fn new(
        parent_recorder: &'a SingleWriterRecorder,
        histogram: *mut Histogram<u64>,
        guard: PhaseFlipGuard<'b>,
    ) -> Self {
        SingleWriterLockingSample {
            parent_recorder,
            histogram: AtomicPtr::new(histogram),
            guard,
        }
    }

    pub fn resample(self) -> Self {
        unsafe {
            let to_swap = self.histogram.load(Ordering::Acquire);
            (*to_swap).reset();
            let res = self.parent_recorder.perform_interval_sample(to_swap, &self.guard);
            self.histogram.store(res, Ordering::Release);
            self
        }
    }

    pub fn histogram(&self) -> &Histogram<u64> {
        unsafe { &*self.histogram.load(Ordering::Acquire) }
    }

    /// Return a read-only view of the most recently sampled histogram.
    ///
    /// This is an alias for [`histogram`](Self::histogram) with the same naming
    /// convention as the concurrent recorder sample types.
    pub fn snapshot(&self) -> &Histogram<u64> {
        self.histogram()
    }
}

impl<'a, 'b: 'a> Drop for SingleWriterLockingSample<'a, 'b> {
    fn drop(&mut self) {
        unsafe {
            self.guard.flip();
            mem::drop(Box::from_raw(self.histogram.load(Ordering::SeqCst)));
        }
    }
}

pub struct SingleWriterDoubleLockingSample<'a, 'b: 'a, P: OverflowPolicy> {
    parent_recorder: &'a SingleWriterDoubleRecorder<P>,
    histogram: AtomicPtr<DoubleHistogramImpl<P>>,
    guard: PhaseFlipGuard<'b>,
}

impl<'a, 'b: 'a, P: OverflowPolicy> SingleWriterDoubleLockingSample<'a, 'b, P> {
    pub(in crate::concurrent) fn new(
        parent_recorder: &'a SingleWriterDoubleRecorder<P>,
        histogram: *mut DoubleHistogramImpl<P>,
        guard: PhaseFlipGuard<'b>,
    ) -> Self {
        SingleWriterDoubleLockingSample {
            parent_recorder,
            histogram: AtomicPtr::new(histogram),
            guard,
        }
    }

    pub fn resample(self) -> Self {
        unsafe {
            let to_swap = self.histogram.load(Ordering::Acquire);
            (*to_swap).reset();
            let res = self.parent_recorder.perform_interval_sample(to_swap, &self.guard);
            self.histogram.store(res, Ordering::Release);
            self
        }
    }

    pub fn histogram(&self) -> &DoubleHistogramImpl<P> {
        unsafe { &*self.histogram.load(Ordering::Acquire) }
    }

    /// Return a read-only view of the most recently sampled histogram.
    ///
    /// This is an alias for [`histogram`](Self::histogram) with the same naming
    /// convention as the concurrent recorder sample types.
    pub fn snapshot(&self) -> &DoubleHistogramImpl<P> {
        self.histogram()
    }
}

impl<'a, 'b: 'a, P: OverflowPolicy> Drop for SingleWriterDoubleLockingSample<'a, 'b, P> {
    fn drop(&mut self) {
        unsafe {
            self.guard.flip();
            mem::drop(Box::from_raw(self.histogram.load(Ordering::SeqCst)));
        }
    }
}
