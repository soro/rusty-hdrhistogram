use crate::concurrent::double_histogram::ConcurrentDoubleHistogram;
use crate::concurrent::interval_sample::{
    DoubleIntervalSample, FixedIntervalSample, IntervalSampleCore, ResizableIntervalSample, SingleWriterDoubleIntervalSample,
    SingleWriterIntervalSample,
};
use crate::concurrent::recordable_histogram::RecordableHistogram;
use crate::concurrent::resizable_histogram::ResizableConcurrentHistogram;
use crate::concurrent::static_histogram::FixedConcurrentHistogram;
use crate::concurrent::writer_reader_phaser::{PhaseFlipGuard, WriterReaderPhaser};
use crate::core::*;
use crate::st::{DoubleHistogram, Histogram};
use std::marker::PhantomData;
use std::mem;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

const DEFAULT_SIGNIFICANT_VALUE_DIGITS: u8 = 3;

static REPORTER_INSTANCE_SEQUENCER: AtomicUsize = AtomicUsize::new(0);
fn get_instance_id() -> usize {
    REPORTER_INSTANCE_SEQUENCER.fetch_add(1, Ordering::Relaxed)
}

pub(crate) struct Recorder<T: RecordableHistogram> {
    instance_id: usize,
    core: RecorderCore<T>,
}

pub(crate) struct RecorderCore<T> {
    recording_phaser: WriterReaderPhaser,
    active_histogram: AtomicPtr<T>,
}

pub struct FixedRecorder {
    inner: Recorder<FixedConcurrentHistogram>,
}

pub struct FixedRecorderBuilder {
    lowest_discernible_value: u64,
    highest_trackable_value: u64,
    significant_value_digits: u8,
}

pub struct ResizableRecorder {
    inner: Recorder<ResizableConcurrentHistogram>,
}

pub struct ResizableRecorderBuilder {
    lowest_discernible_value: u64,
    highest_trackable_value: u64,
    significant_value_digits: u8,
    auto_resize: bool,
}

/// The unique recording handle for a single-writer integer recorder.
///
/// Recording requires mutable access to this handle. Move the corresponding
/// [`SingleWriterSampler`] to the sampling thread when intervals are needed.
/// The builder allocates both histogram buffers before returning the handles.
///
/// Recording without a mutable writer does not compile:
///
/// ```compile_fail
/// use hdrhistogram::SingleWriterRecorder;
///
/// let (recorder, _sampler) = SingleWriterRecorder::builder().build().unwrap();
/// recorder.record_value(42).unwrap();
/// ```
///
/// The writer is deliberately not cloneable:
///
/// ```compile_fail
/// use hdrhistogram::SingleWriterRecorder;
///
/// let (recorder, _sampler) = SingleWriterRecorder::builder().build().unwrap();
/// let _second_writer = recorder.clone();
/// ```
pub struct SingleWriterRecorder {
    instance_id: usize,
    core: Arc<RecorderCore<Histogram>>,
}

/// The unique sampling handle paired with a [`SingleWriterRecorder`].
///
/// An active [`SingleWriterIntervalSample`] mutably borrows this handle, so a
/// second sampling operation cannot begin until that sample is resampled or
/// dropped.
pub struct SingleWriterSampler {
    core: Arc<RecorderCore<Histogram>>,
    inactive_histogram: Option<Box<Histogram>>,
}

pub struct SingleWriterRecorderBuilder {
    lowest_discernible_value: u64,
    highest_trackable_value: u64,
    significant_value_digits: u8,
    auto_resize: bool,
}

pub type SaturatingDoubleRecorder = DoubleRecorder<SaturateOnOverflow>;
pub type SaturatingSingleWriterDoubleRecorder = SingleWriterDoubleRecorder<SaturateOnOverflow>;
pub type SaturatingSingleWriterDoubleSampler = SingleWriterDoubleSampler<SaturateOnOverflow>;

pub struct DoubleRecorder<P: OverflowPolicy = ThrowOnOverflow> {
    instance_id: usize,
    core: RecorderCore<ConcurrentDoubleHistogram<P>>,
}

pub struct DoubleRecorderBuilder<P: OverflowPolicy = ThrowOnOverflow> {
    highest_to_lowest_value_ratio: u64,
    number_of_significant_value_digits: u8,
    auto_resize: bool,
    _policy: PhantomData<P>,
}

/// The unique recording handle for a single-writer floating-point recorder.
///
/// Recording requires mutable access to this handle. Move the corresponding
/// [`SingleWriterDoubleSampler`] to the sampling thread when intervals are
/// needed. The builder allocates both histogram buffers before returning the
/// handles.
pub struct SingleWriterDoubleRecorder<P: OverflowPolicy = ThrowOnOverflow> {
    instance_id: usize,
    core: Arc<RecorderCore<DoubleHistogram<P>>>,
}

/// The unique sampling handle paired with a [`SingleWriterDoubleRecorder`].
///
/// Each recycled buffer retains its own expanded capacity. Sampling never
/// reads mutable range settings from the live active histogram.
pub struct SingleWriterDoubleSampler<P: OverflowPolicy = ThrowOnOverflow> {
    core: Arc<RecorderCore<DoubleHistogram<P>>>,
    inactive_histogram: Option<Box<DoubleHistogram<P>>>,
}

pub struct SingleWriterDoubleRecorderBuilder<P: OverflowPolicy = ThrowOnOverflow> {
    highest_to_lowest_value_ratio: u64,
    number_of_significant_value_digits: u8,
    auto_resize: bool,
    _policy: PhantomData<P>,
}

impl FixedRecorderBuilder {
    pub fn new() -> Self {
        FixedRecorderBuilder {
            lowest_discernible_value: 1,
            highest_trackable_value: 2,
            significant_value_digits: DEFAULT_SIGNIFICANT_VALUE_DIGITS,
        }
    }

    /// Set the number of decimal significant digits retained by the recorder.
    ///
    /// The default is `3`, which gives roughly three significant decimal digits
    /// of precision.
    pub fn significant_digits(mut self, significant_value_digits: u8) -> Self {
        self.significant_value_digits = significant_value_digits;
        self
    }

    pub fn lowest_discernible_value(mut self, lowest_discernible_value: u64) -> Self {
        self.lowest_discernible_value = lowest_discernible_value;
        self
    }

    pub fn highest_trackable_value(mut self, highest_trackable_value: u64) -> Self {
        self.highest_trackable_value = highest_trackable_value;
        self
    }

    pub fn build(self) -> Result<FixedRecorder, CreationError> {
        FixedConcurrentHistogram::builder()
            .lowest_discernible_value(self.lowest_discernible_value)
            .highest_trackable_value(self.highest_trackable_value)
            .significant_digits(self.significant_value_digits)
            .build()
            .map(FixedRecorder::from_histogram)
    }
}

impl ResizableRecorderBuilder {
    pub fn new() -> Self {
        ResizableRecorderBuilder {
            lowest_discernible_value: 1,
            highest_trackable_value: 2,
            significant_value_digits: DEFAULT_SIGNIFICANT_VALUE_DIGITS,
            auto_resize: true,
        }
    }

    /// Set the number of decimal significant digits retained by the recorder.
    ///
    /// The default is `3`, which gives roughly three significant decimal digits
    /// of precision.
    pub fn significant_digits(mut self, significant_value_digits: u8) -> Self {
        self.significant_value_digits = significant_value_digits;
        self
    }

    pub fn lowest_discernible_value(mut self, lowest_discernible_value: u64) -> Self {
        self.lowest_discernible_value = lowest_discernible_value;
        self
    }

    /// Set the recorder's configured highest trackable value.
    ///
    /// When auto-resize is enabled, this is the initial range and the recorder
    /// can grow later to accommodate larger recorded values.
    pub fn highest_trackable_value(mut self, highest_trackable_value: u64) -> Self {
        self.highest_trackable_value = highest_trackable_value;
        self
    }

    pub fn auto_resize(mut self, auto_resize: bool) -> Self {
        self.auto_resize = auto_resize;
        self
    }

    pub fn build(self) -> Result<ResizableRecorder, CreationError> {
        let histogram = ResizableConcurrentHistogram::builder()
            .lowest_discernible_value(self.lowest_discernible_value)
            .highest_trackable_value(self.highest_trackable_value)
            .significant_digits(self.significant_value_digits)
            .auto_resize(self.auto_resize)
            .build()?;
        Ok(ResizableRecorder::from_histogram(histogram))
    }
}

impl SingleWriterRecorderBuilder {
    pub fn new() -> Self {
        SingleWriterRecorderBuilder {
            lowest_discernible_value: 1,
            highest_trackable_value: 2,
            significant_value_digits: DEFAULT_SIGNIFICANT_VALUE_DIGITS,
            auto_resize: true,
        }
    }

    /// Set the number of decimal significant digits retained by the recorder.
    ///
    /// The default is `3`, which gives roughly three significant decimal digits
    /// of precision.
    pub fn significant_digits(mut self, significant_value_digits: u8) -> Self {
        self.significant_value_digits = significant_value_digits;
        self
    }

    pub fn lowest_discernible_value(mut self, lowest_discernible_value: u64) -> Self {
        self.lowest_discernible_value = lowest_discernible_value;
        self
    }

    /// Set the recorder's configured highest trackable value.
    ///
    /// When auto-resize is enabled, this is the initial range and the recorder
    /// can grow later to accommodate larger recorded values.
    pub fn highest_trackable_value(mut self, highest_trackable_value: u64) -> Self {
        self.highest_trackable_value = highest_trackable_value;
        self
    }

    pub fn auto_resize(mut self, auto_resize: bool) -> Self {
        self.auto_resize = auto_resize;
        self
    }

    /// Allocate both buffers and return the unique writer and sampler handles.
    pub fn build(self) -> Result<(SingleWriterRecorder, SingleWriterSampler), CreationError> {
        let histogram = Histogram::builder()
            .lowest_discernible_value(self.lowest_discernible_value)
            .highest_trackable_value(self.highest_trackable_value)
            .significant_digits(self.significant_value_digits)
            .auto_resize(self.auto_resize)
            .build()?;
        SingleWriterRecorder::from_histogram(histogram)
    }
}

impl<P: OverflowPolicy> DoubleRecorderBuilder<P> {
    pub fn new() -> Self {
        DoubleRecorderBuilder {
            highest_to_lowest_value_ratio: 2,
            number_of_significant_value_digits: DEFAULT_SIGNIFICANT_VALUE_DIGITS,
            auto_resize: true,
            _policy: PhantomData,
        }
    }

    /// Set the number of decimal significant digits retained by the recorder.
    ///
    /// The default is `3`, which gives roughly three significant decimal digits
    /// of precision.
    pub fn significant_digits(mut self, number_of_significant_value_digits: u8) -> Self {
        self.number_of_significant_value_digits = number_of_significant_value_digits;
        self
    }

    pub fn highest_to_lowest_value_ratio(mut self, highest_to_lowest_value_ratio: u64) -> Self {
        self.highest_to_lowest_value_ratio = highest_to_lowest_value_ratio;
        self
    }

    pub fn auto_resize(mut self, auto_resize: bool) -> Self {
        self.auto_resize = auto_resize;
        self
    }

    pub fn build(self) -> Result<DoubleRecorder<P>, DoubleCreationError> {
        let histogram = ConcurrentDoubleHistogram::<P>::builder()
            .highest_to_lowest_value_ratio(self.highest_to_lowest_value_ratio)
            .significant_digits(self.number_of_significant_value_digits)
            .auto_resize(self.auto_resize)
            .build()?;
        Ok(DoubleRecorder::from_histogram(histogram))
    }
}

impl<P: OverflowPolicy> SingleWriterDoubleRecorderBuilder<P> {
    pub fn new() -> Self {
        SingleWriterDoubleRecorderBuilder {
            highest_to_lowest_value_ratio: 2,
            number_of_significant_value_digits: DEFAULT_SIGNIFICANT_VALUE_DIGITS,
            auto_resize: true,
            _policy: PhantomData,
        }
    }

    /// Set the number of decimal significant digits retained by the recorder.
    ///
    /// The default is `3`, which gives roughly three significant decimal digits
    /// of precision.
    pub fn significant_digits(mut self, number_of_significant_value_digits: u8) -> Self {
        self.number_of_significant_value_digits = number_of_significant_value_digits;
        self
    }

    pub fn highest_to_lowest_value_ratio(mut self, highest_to_lowest_value_ratio: u64) -> Self {
        self.highest_to_lowest_value_ratio = highest_to_lowest_value_ratio;
        self
    }

    pub fn auto_resize(mut self, auto_resize: bool) -> Self {
        self.auto_resize = auto_resize;
        self
    }

    /// Allocate both buffers and return the unique writer and sampler handles.
    pub fn build(self) -> Result<(SingleWriterDoubleRecorder<P>, SingleWriterDoubleSampler<P>), DoubleCreationError> {
        let histogram = DoubleHistogram::<P>::builder()
            .highest_to_lowest_value_ratio(self.highest_to_lowest_value_ratio)
            .significant_digits(self.number_of_significant_value_digits)
            .auto_resize(self.auto_resize)
            .build()?;
        SingleWriterDoubleRecorder::from_histogram(histogram)
    }
}

impl<T> RecorderCore<T> {
    fn from_histogram(histogram: T) -> RecorderCore<T> {
        RecorderCore {
            recording_phaser: WriterReaderPhaser::new(),
            active_histogram: AtomicPtr::new(Box::into_raw(Box::new(histogram))),
        }
    }

    #[inline(always)]
    fn active(&self) -> *mut T {
        self.active_histogram.load(Ordering::Acquire)
    }

    fn reader_lock(&self) -> PhaseFlipGuard<'_> {
        self.recording_phaser.reader_lock()
    }

    #[inline(always)]
    fn begin_writer_critical_section(&self) -> crate::concurrent::writer_reader_phaser::WriterCriticalSectionGuard<'_> {
        self.recording_phaser.begin_writer_critical_section()
    }

    fn swap_active(&self, inactive_histogram: *mut T) -> *mut T {
        // The SeqCst swap publishes the cleared/fresh inactive histogram to new
        // writers. Writer-side Acquire loads make the histogram reset visible
        // before they record into the new active histogram.
        self.active_histogram.swap(inactive_histogram, Ordering::SeqCst)
    }
}

impl<T> Drop for RecorderCore<T> {
    fn drop(&mut self) {
        unsafe {
            self.recording_phaser.reader_lock().flip();
            mem::drop(Box::from_raw(self.active_histogram.load(Ordering::SeqCst)));
        }
    }
}

impl<T: RecordableHistogram> Recorder<T> {
    pub(crate) fn from_histogram(mut histogram: T) -> Recorder<T> {
        histogram.meta_data_mut().set_start_now();
        Recorder {
            instance_id: get_instance_id(),
            core: RecorderCore::from_histogram(histogram),
        }
    }

    #[inline(always)]
    pub(crate) fn record_value(&self, value: u64) -> Result<(), RecordError> {
        unsafe {
            let _csg = self.core.begin_writer_critical_section();
            (*self.core.active()).record_value(value)
        }
    }

    #[inline(always)]
    pub(crate) fn record_value_with_count(&self, value: u64, count: u64) -> Result<(), RecordError> {
        unsafe {
            let _csg = self.core.begin_writer_critical_section();
            (*self.core.active()).record_value_with_count(value, count)
        }
    }

    #[inline]
    pub(crate) fn record_value_with_count_and_expected_interval(
        &self,
        value: u64,
        count: u64,
        expected_interval_between_value_samples: u64,
    ) -> Result<(), RecordError> {
        unsafe {
            let _csg = self.core.begin_writer_critical_section();
            let active_histogram = &*self.core.active();
            active_histogram.record_value_with_count(value, count)?;
            if expected_interval_between_value_samples != 0 && value > expected_interval_between_value_samples {
                let mut missing_value = value - expected_interval_between_value_samples;
                while missing_value >= expected_interval_between_value_samples {
                    active_histogram.record_value_with_count(missing_value, count)?;
                    missing_value -= expected_interval_between_value_samples;
                }
            }

            Ok(())
        }
    }

    #[inline]
    pub(crate) fn record_value_with_expected_interval(
        &self,
        value: u64,
        expected_interval_between_value_samples: u64,
    ) -> Result<(), RecordError> {
        self.record_value_with_count_and_expected_interval(value, 1, expected_interval_between_value_samples)
    }

    pub(crate) fn begin_interval_sample<'a>(&'a self) -> IntervalSampleCore<'a, 'a, T> {
        let pfg = self.core.reader_lock();
        let settings = unsafe { (&*self.core.active()).settings() };
        let fresh_histogram = Box::new(T::fresh(&settings).unwrap());
        let sample = self.perform_interval_sample(Box::into_raw(fresh_histogram), &pfg);
        IntervalSampleCore::new(self, sample, pfg)
    }

    pub(in crate::concurrent) fn perform_interval_sample<'a>(&self, inactive_histogram: *mut T, flip_guard: &PhaseFlipGuard<'a>) -> *mut T {
        let now = SystemTime::now();
        unsafe { (*inactive_histogram).meta_data_mut().set_start_timestamp(now) };
        let active_histogram = self.core.swap_active(inactive_histogram);

        flip_guard.flip();

        unsafe { (*active_histogram).meta_data_mut().set_end_timestamp(now) };
        active_histogram
    }
}

macro_rules! impl_integer_recorder {
    ($recorder:ident, $histogram:ty, $sample:ident) => {
        impl $recorder {
            pub fn from_histogram(histogram: $histogram) -> Self {
                $recorder {
                    inner: Recorder::from_histogram(histogram),
                }
            }

            #[inline(always)]
            pub fn record_value(&self, value: u64) -> Result<(), RecordError> {
                self.inner.record_value(value)
            }

            #[inline(always)]
            pub fn record_value_with_count(&self, value: u64, count: u64) -> Result<(), RecordError> {
                self.inner.record_value_with_count(value, count)
            }

            #[inline]
            pub fn record_value_with_count_and_expected_interval(
                &self,
                value: u64,
                count: u64,
                expected_interval_between_value_samples: u64,
            ) -> Result<(), RecordError> {
                self.inner
                    .record_value_with_count_and_expected_interval(value, count, expected_interval_between_value_samples)
            }

            #[inline]
            pub fn record_value_with_expected_interval(
                &self,
                value: u64,
                expected_interval_between_value_samples: u64,
            ) -> Result<(), RecordError> {
                self.inner
                    .record_value_with_expected_interval(value, expected_interval_between_value_samples)
            }

            /// Begin sampling an interval from this recorder.
            ///
            /// The returned value owns the recorder's exclusive sampling lease.
            /// Writers continue recording into the next interval while it is
            /// alive; resampling may wait for writer calls that were already in
            /// flight.
            pub fn begin_interval_sample<'a>(&'a self) -> $sample<'a, 'a> {
                $sample::new(self.inner.begin_interval_sample())
            }
        }
    };
}

impl_integer_recorder!(FixedRecorder, FixedConcurrentHistogram, FixedIntervalSample);
impl_integer_recorder!(ResizableRecorder, ResizableConcurrentHistogram, ResizableIntervalSample);

impl FixedRecorder {
    pub fn builder() -> FixedRecorderBuilder {
        FixedRecorderBuilder::new()
    }
}

impl ResizableRecorder {
    pub fn builder() -> ResizableRecorderBuilder {
        ResizableRecorderBuilder::new()
    }
}

impl SingleWriterRecorder {
    pub fn builder() -> SingleWriterRecorderBuilder {
        SingleWriterRecorderBuilder::new()
    }

    /// Create the unique writer and sampler handles around an existing
    /// histogram.
    pub fn from_histogram(mut histogram: Histogram) -> Result<(Self, SingleWriterSampler), CreationError> {
        let inactive_histogram = Box::new(histogram.empty_like_for_recorder());
        histogram.meta_data.set_start_now();
        let core = Arc::new(RecorderCore::from_histogram(histogram));
        let recorder = SingleWriterRecorder {
            instance_id: get_instance_id(),
            core: Arc::clone(&core),
        };
        let sampler = SingleWriterSampler {
            core,
            inactive_histogram: Some(inactive_histogram),
        };
        Ok((recorder, sampler))
    }

    #[inline(always)]
    pub fn record_value(&mut self, value: u64) -> Result<(), RecordError> {
        unsafe {
            // SAFETY: mutable access to the unique writer prevents another
            // writer from mutating the active plain histogram. The phaser
            // guard prevents the sampler from exposing or recycling the
            // loaded pointer until this mutation has completed.
            let _writer = self.core.begin_writer_critical_section();
            (*self.core.active()).record_value(value)
        }
    }

    #[inline(always)]
    pub fn record_value_with_count(&mut self, value: u64, count: u64) -> Result<(), RecordError> {
        unsafe {
            let _writer = self.core.begin_writer_critical_section();
            (*self.core.active()).record_value_with_count(value, count)
        }
    }

    #[inline]
    pub fn record_value_with_count_and_expected_interval(
        &mut self,
        value: u64,
        count: u64,
        expected_interval_between_value_samples: u64,
    ) -> Result<(), RecordError> {
        unsafe {
            let _writer = self.core.begin_writer_critical_section();
            (*self.core.active()).record_value_with_count_and_expected_interval(value, count, expected_interval_between_value_samples)
        }
    }

    #[inline]
    pub fn record_value_with_expected_interval(
        &mut self,
        value: u64,
        expected_interval_between_value_samples: u64,
    ) -> Result<(), RecordError> {
        self.record_value_with_count_and_expected_interval(value, 1, expected_interval_between_value_samples)
    }
}

impl SingleWriterSampler {
    /// Begin sampling an interval from this recorder.
    ///
    /// The returned value borrows this unique sampler and owns the inactive
    /// buffer until it is resampled or dropped. The writer continues recording
    /// into the next interval after the handoff.
    pub fn begin_interval_sample(&mut self) -> SingleWriterIntervalSample<'_> {
        let inactive_histogram = self
            .inactive_histogram
            .take()
            .expect("single-writer sampler must own exactly one inactive histogram");
        let sample = self.perform_interval_sample(inactive_histogram);
        SingleWriterIntervalSample::new(self, sample)
    }

    pub(in crate::concurrent) fn perform_interval_sample(&mut self, mut inactive_histogram: Box<Histogram>) -> Box<Histogram> {
        inactive_histogram.reset();
        let now = SystemTime::now();
        inactive_histogram.meta_data.set_start_timestamp(now);

        let flip_guard = self.core.reader_lock();
        let active_histogram = self.core.swap_active(Box::into_raw(inactive_histogram));
        flip_guard.flip();

        // SAFETY: the pointer was formerly owned by RecorderCore. The swap
        // removed it from the active slot, and the phase flip waited for every
        // writer that could have loaded it before ownership is reconstructed.
        let mut sampled_histogram = unsafe { Box::from_raw(active_histogram) };
        sampled_histogram.meta_data.set_end_timestamp(now);
        sampled_histogram
    }

    pub(in crate::concurrent) fn return_inactive(&mut self, histogram: Box<Histogram>) {
        assert!(
            self.inactive_histogram.replace(histogram).is_none(),
            "single-writer sampler already owns an inactive histogram"
        );
    }
}

impl<P: OverflowPolicy> DoubleRecorder<P> {
    pub fn builder() -> DoubleRecorderBuilder<P> {
        DoubleRecorderBuilder::new()
    }

    pub fn from_histogram(mut histogram: ConcurrentDoubleHistogram<P>) -> Self {
        histogram.meta_data_mut().set_start_now();
        DoubleRecorder {
            instance_id: get_instance_id(),
            core: RecorderCore::from_histogram(histogram),
        }
    }

    pub fn record_value(&self, value: f64) -> Result<(), RecordError> {
        unsafe {
            let _csg = self.core.begin_writer_critical_section();
            (*self.core.active()).record_value(value)
        }
    }

    pub fn record_value_with_count(&self, value: f64, count: u64) -> Result<(), RecordError> {
        unsafe {
            let _csg = self.core.begin_writer_critical_section();
            (*self.core.active()).record_value_with_count(value, count)
        }
    }

    #[inline]
    pub fn record_value_with_count_and_expected_interval(
        &self,
        value: f64,
        count: u64,
        expected_interval_between_value_samples: f64,
    ) -> Result<(), RecordError> {
        unsafe {
            let _csg = self.core.begin_writer_critical_section();
            let active = &*self.core.active();
            active.record_value_with_count(value, count)?;
            if expected_interval_between_value_samples <= 0.0 {
                return Ok(());
            }
            let mut missing_value = value - expected_interval_between_value_samples;
            while missing_value >= expected_interval_between_value_samples {
                active.record_value_with_count(missing_value, count)?;
                missing_value -= expected_interval_between_value_samples;
            }
            Ok(())
        }
    }

    #[inline]
    pub fn record_value_with_expected_interval(&self, value: f64, expected_interval_between_value_samples: f64) -> Result<(), RecordError> {
        self.record_value_with_count_and_expected_interval(value, 1, expected_interval_between_value_samples)
    }

    /// Begin sampling an interval from this recorder.
    ///
    /// The returned value owns the recorder's exclusive sampling lease. Writers
    /// continue recording into the next interval while it is alive; resampling
    /// may wait for writer calls that were already in flight.
    pub fn begin_interval_sample<'a>(&'a self) -> DoubleIntervalSample<'a, 'a, P> {
        let pfg = self.core.reader_lock();
        let active = unsafe { &*self.core.active() };
        let fresh_histogram = ConcurrentDoubleHistogram::<P>::builder()
            .highest_to_lowest_value_ratio(active.get_highest_to_lowest_value_ratio())
            .significant_digits(active.get_number_of_significant_value_digits())
            .auto_resize(active.is_auto_resize())
            .build()
            .expect("active double recorder histogram settings must be reusable");
        let sample = self.perform_interval_sample(Box::into_raw(Box::new(fresh_histogram)), &pfg);
        DoubleIntervalSample::new(self, sample, pfg)
    }

    pub(in crate::concurrent) fn perform_interval_sample<'a>(
        &self,
        inactive_histogram: *mut ConcurrentDoubleHistogram<P>,
        flip_guard: &PhaseFlipGuard<'a>,
    ) -> *mut ConcurrentDoubleHistogram<P> {
        let now = SystemTime::now();
        unsafe { (*inactive_histogram).meta_data_mut().set_start_timestamp(now) };
        let active_histogram = self.core.swap_active(inactive_histogram);
        flip_guard.flip();
        unsafe { (*active_histogram).meta_data_mut().set_end_timestamp(now) };
        active_histogram
    }
}

impl<P: OverflowPolicy> SingleWriterDoubleRecorder<P> {
    pub fn builder() -> SingleWriterDoubleRecorderBuilder<P> {
        SingleWriterDoubleRecorderBuilder::new()
    }

    /// Create the unique writer and sampler handles around an existing
    /// histogram.
    pub fn from_histogram(mut histogram: DoubleHistogram<P>) -> Result<(Self, SingleWriterDoubleSampler<P>), DoubleCreationError> {
        let inactive_histogram = Box::new(histogram.empty_like_for_recorder());
        histogram.meta_data_mut().set_start_now();
        let core = Arc::new(RecorderCore::from_histogram(histogram));
        let recorder = SingleWriterDoubleRecorder {
            instance_id: get_instance_id(),
            core: Arc::clone(&core),
        };
        let sampler = SingleWriterDoubleSampler {
            core,
            inactive_histogram: Some(inactive_histogram),
        };
        Ok((recorder, sampler))
    }

    #[inline(always)]
    pub fn record_value(&mut self, value: f64) -> Result<(), RecordError> {
        unsafe {
            // SAFETY: mutable access to the unique writer prevents another
            // writer from mutating the active plain histogram. The phaser
            // guard prevents buffer reuse until this mutation completes.
            let _writer = self.core.begin_writer_critical_section();
            (*self.core.active()).record_value(value)
        }
    }

    #[inline(always)]
    pub fn record_value_with_count(&mut self, value: f64, count: u64) -> Result<(), RecordError> {
        unsafe {
            let _writer = self.core.begin_writer_critical_section();
            (*self.core.active()).record_value_with_count(value, count)
        }
    }

    #[inline]
    pub fn record_value_with_count_and_expected_interval(
        &mut self,
        value: f64,
        count: u64,
        expected_interval_between_value_samples: f64,
    ) -> Result<(), RecordError> {
        unsafe {
            let _writer = self.core.begin_writer_critical_section();
            let active = &mut *self.core.active();
            active.record_value_with_count(value, count)?;
            if expected_interval_between_value_samples <= 0.0 {
                return Ok(());
            }
            let mut missing_value = value - expected_interval_between_value_samples;
            while missing_value >= expected_interval_between_value_samples {
                active.record_value_with_count(missing_value, count)?;
                missing_value -= expected_interval_between_value_samples;
            }
            Ok(())
        }
    }

    #[inline]
    pub fn record_value_with_expected_interval(
        &mut self,
        value: f64,
        expected_interval_between_value_samples: f64,
    ) -> Result<(), RecordError> {
        self.record_value_with_count_and_expected_interval(value, 1, expected_interval_between_value_samples)
    }
}

impl<P: OverflowPolicy> SingleWriterDoubleSampler<P> {
    /// Begin sampling an interval from this recorder.
    ///
    /// The returned value borrows this unique sampler and owns the inactive
    /// buffer until it is resampled or dropped. The writer continues recording
    /// into the next interval after the handoff.
    pub fn begin_interval_sample(&mut self) -> SingleWriterDoubleIntervalSample<'_, P> {
        let inactive_histogram = self
            .inactive_histogram
            .take()
            .expect("single-writer double sampler must own exactly one inactive histogram");
        let sample = self.perform_interval_sample(inactive_histogram);
        SingleWriterDoubleIntervalSample::new(self, sample)
    }

    pub(in crate::concurrent) fn perform_interval_sample(
        &mut self,
        mut inactive_histogram: Box<DoubleHistogram<P>>,
    ) -> Box<DoubleHistogram<P>> {
        inactive_histogram.reset();
        let now = SystemTime::now();
        inactive_histogram.meta_data_mut().set_start_timestamp(now);

        let flip_guard = self.core.reader_lock();
        let active_histogram = self.core.swap_active(Box::into_raw(inactive_histogram));
        flip_guard.flip();

        // SAFETY: the swap removed this pointer from RecorderCore ownership,
        // and the phase flip waited for all writers that could still use it.
        let mut sampled_histogram = unsafe { Box::from_raw(active_histogram) };
        sampled_histogram.meta_data_mut().set_end_timestamp(now);
        sampled_histogram
    }

    pub(in crate::concurrent) fn return_inactive(&mut self, histogram: Box<DoubleHistogram<P>>) {
        assert!(
            self.inactive_histogram.replace(histogram).is_none(),
            "single-writer double sampler already owns an inactive histogram"
        );
    }
}
