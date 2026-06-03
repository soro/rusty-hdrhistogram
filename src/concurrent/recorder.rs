use crate::concurrent::double_histogram::ConcurrentDoubleHistogramWithPolicy;
use crate::concurrent::interval_sample::{
    DoubleIntervalSample, FixedIntervalSample, IntervalSampleCore, ResizableIntervalSample, SingleWriterDoubleIntervalSample,
    SingleWriterIntervalSample,
};
use crate::concurrent::recordable_histogram::RecordableHistogram;
use crate::concurrent::resizable_histogram::ResizableConcurrentHistogram;
use crate::concurrent::static_histogram::FixedConcurrentHistogram;
use crate::concurrent::writer_reader_phaser::{PhaseFlipGuard, WriterReaderPhaser};
use crate::core::*;
use crate::st::{DoubleHistogramWithPolicy, Histogram};
use std::marker::PhantomData;
use std::mem;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
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

/// Recorder optimized for exactly one recording thread plus optional sampling.
///
/// Recording is kept as cheap as possible. Sampling waits for the current record
/// operation to finish and may be delayed indefinitely by a continuously active
/// writer; this is intentional for the single-writer variant.
pub struct SingleWriterRecorder {
    instance_id: usize,
    core: SingleWriterCore<Histogram>,
    inactive_settings: HistogramSettings,
    inactive_integer_to_double_value_conversion_ratio: f64,
}

pub struct SingleWriterRecorderBuilder {
    lowest_discernible_value: u64,
    highest_trackable_value: u64,
    significant_value_digits: u8,
    auto_resize: bool,
}

pub type DoubleRecorder = DoubleRecorderWithPolicy<ThrowOnOverflow>;
pub type SaturatingDoubleRecorder = DoubleRecorderWithPolicy<SaturateOnOverflow>;
pub type SingleWriterDoubleRecorder = SingleWriterDoubleRecorderWithPolicy<ThrowOnOverflow>;
pub type SaturatingSingleWriterDoubleRecorder = SingleWriterDoubleRecorderWithPolicy<SaturateOnOverflow>;

pub struct DoubleRecorderWithPolicy<P: OverflowPolicy> {
    instance_id: usize,
    core: RecorderCore<ConcurrentDoubleHistogramWithPolicy<P>>,
}

pub struct DoubleRecorderBuilder<P: OverflowPolicy> {
    highest_to_lowest_value_ratio: u64,
    number_of_significant_value_digits: u8,
    auto_resize: bool,
    _policy: PhantomData<P>,
}

/// Double recorder optimized for exactly one recording thread plus optional sampling.
///
/// Recording is kept as cheap as possible. Sampling waits for the current record
/// operation to finish and may be delayed indefinitely by a continuously active
/// writer; this is intentional for the single-writer variant.
pub struct SingleWriterDoubleRecorderWithPolicy<P: OverflowPolicy> {
    instance_id: usize,
    core: SingleWriterCore<DoubleHistogramWithPolicy<P>>,
    inactive_highest_to_lowest_value_ratio: u64,
    inactive_number_of_significant_value_digits: u8,
    inactive_auto_resize: bool,
}

pub struct SingleWriterDoubleRecorderBuilder<P: OverflowPolicy> {
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
        FixedConcurrentHistogram::with_low_high_sigvdig(
            self.lowest_discernible_value,
            self.highest_trackable_value,
            self.significant_value_digits,
        )
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
        let histogram = ResizableConcurrentHistogram::with_low_high_sigvdig(
            self.lowest_discernible_value,
            self.highest_trackable_value,
            self.significant_value_digits,
        )?;
        histogram.set_auto_resize(self.auto_resize);
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

    pub fn build(self) -> Result<SingleWriterRecorder, CreationError> {
        let mut histogram = Histogram::with_low_high_sigvdig(
            self.lowest_discernible_value,
            self.highest_trackable_value,
            self.significant_value_digits,
        )?;
        histogram.set_auto_resize(self.auto_resize);
        Ok(SingleWriterRecorder::from_histogram(histogram))
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

    pub fn build(self) -> Result<DoubleRecorderWithPolicy<P>, DoubleCreationError> {
        let histogram = ConcurrentDoubleHistogramWithPolicy::<P>::with_highest_to_lowest_value_ratio(
            self.highest_to_lowest_value_ratio,
            self.number_of_significant_value_digits,
        )?;
        histogram.set_auto_resize(self.auto_resize);
        Ok(DoubleRecorderWithPolicy::from_histogram(histogram))
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

    pub fn build(self) -> Result<SingleWriterDoubleRecorderWithPolicy<P>, DoubleCreationError> {
        let mut histogram = DoubleHistogramWithPolicy::<P>::with_highest_to_lowest_value_ratio(
            self.highest_to_lowest_value_ratio,
            self.number_of_significant_value_digits,
        )?;
        histogram.set_auto_resize(self.auto_resize);
        Ok(SingleWriterDoubleRecorderWithPolicy::from_histogram(histogram))
    }
}

impl<T> RecorderCore<T> {
    fn from_histogram(histogram: T) -> RecorderCore<T> {
        RecorderCore {
            recording_phaser: WriterReaderPhaser::new(),
            active_histogram: AtomicPtr::new(Box::into_raw(Box::new(histogram))),
        }
    }

    fn active(&self) -> *mut T {
        self.active_histogram.load(Ordering::Acquire)
    }

    fn reader_lock(&self) -> PhaseFlipGuard<'_> {
        self.recording_phaser.reader_lock()
    }

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

struct SingleWriterCore<T> {
    recorder: RecorderCore<T>,
    state: AtomicUsize,
}

struct SingleWriterRecordingGuard<'a> {
    state: &'a AtomicUsize,
}

struct SingleWriterSamplingGuard<'a> {
    state: &'a AtomicUsize,
}

const SINGLE_WRITER_IDLE: usize = 0;
const SINGLE_WRITER_RECORDING: usize = 1;
const SINGLE_WRITER_SAMPLING: usize = 2;

impl<T> SingleWriterCore<T> {
    fn from_histogram(histogram: T) -> Self {
        SingleWriterCore {
            recorder: RecorderCore::from_histogram(histogram),
            state: AtomicUsize::new(SINGLE_WRITER_IDLE),
        }
    }

    fn active(&self) -> *mut T {
        self.recorder.active()
    }

    #[inline(always)]
    fn active_after_recording_guard(&self) -> *mut T {
        // begin_recording's Acquire CAS synchronizes with the sampler's Release
        // store after swap_active(), so the active pointer can be loaded relaxed.
        self.recorder.active_histogram.load(Ordering::Relaxed)
    }

    fn reader_lock(&self) -> PhaseFlipGuard<'_> {
        self.recorder.reader_lock()
    }

    fn swap_active(&self, inactive_histogram: *mut T) -> *mut T {
        self.recorder.swap_active(inactive_histogram)
    }

    #[inline(always)]
    fn begin_recording(&self) -> SingleWriterRecordingGuard<'_> {
        loop {
            match self
                .state
                .compare_exchange(SINGLE_WRITER_IDLE, SINGLE_WRITER_RECORDING, Ordering::Acquire, Ordering::Relaxed)
            {
                Ok(_) => return SingleWriterRecordingGuard { state: &self.state },
                Err(SINGLE_WRITER_RECORDING) => {
                    panic!("single-writer recorder does not support concurrent recording calls");
                }
                Err(SINGLE_WRITER_SAMPLING) => std::hint::spin_loop(),
                Err(_) => unreachable!("invalid single-writer recorder state"),
            }
        }
    }

    fn begin_sampling(&self) -> SingleWriterSamplingGuard<'_> {
        while self
            .state
            .compare_exchange(SINGLE_WRITER_IDLE, SINGLE_WRITER_SAMPLING, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            std::hint::spin_loop();
        }
        SingleWriterSamplingGuard { state: &self.state }
    }
}

impl Drop for SingleWriterRecordingGuard<'_> {
    #[inline(always)]
    fn drop(&mut self) {
        self.state.store(SINGLE_WRITER_IDLE, Ordering::Release);
    }
}

impl Drop for SingleWriterSamplingGuard<'_> {
    #[inline(always)]
    fn drop(&mut self) {
        self.state.store(SINGLE_WRITER_IDLE, Ordering::Release);
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

    pub fn from_histogram(mut histogram: Histogram) -> Self {
        let inactive_settings = histogram.settings().clone();
        let inactive_integer_to_double_value_conversion_ratio = histogram.integer_to_double_value_conversion_ratio();
        histogram.meta_data.set_start_now();
        SingleWriterRecorder {
            instance_id: get_instance_id(),
            core: SingleWriterCore::from_histogram(histogram),
            inactive_settings,
            inactive_integer_to_double_value_conversion_ratio,
        }
    }

    #[inline(always)]
    pub fn record_value(&self, value: u64) -> Result<(), RecordError> {
        unsafe {
            let _access = self.core.begin_recording();
            (*self.core.active_after_recording_guard()).record_value(value)
        }
    }

    #[inline(always)]
    pub fn record_value_with_count(&self, value: u64, count: u64) -> Result<(), RecordError> {
        unsafe {
            let _access = self.core.begin_recording();
            (*self.core.active_after_recording_guard()).record_value_with_count(value, count)
        }
    }

    #[inline]
    pub fn record_value_with_count_and_expected_interval(
        &self,
        value: u64,
        count: u64,
        expected_interval_between_value_samples: u64,
    ) -> Result<(), RecordError> {
        unsafe {
            let _access = self.core.begin_recording();
            (*self.core.active_after_recording_guard()).record_value_with_count_and_expected_interval(
                value,
                count,
                expected_interval_between_value_samples,
            )
        }
    }

    #[inline]
    pub fn record_value_with_expected_interval(&self, value: u64, expected_interval_between_value_samples: u64) -> Result<(), RecordError> {
        self.record_value_with_count_and_expected_interval(value, 1, expected_interval_between_value_samples)
    }

    /// Begin sampling an interval from this recorder.
    ///
    /// The returned value owns the recorder's exclusive sampling lease. The
    /// single writer continues recording into the next interval after the swap;
    /// resampling may briefly wait for an in-flight writer call.
    pub fn begin_interval_sample<'a>(&'a self) -> SingleWriterIntervalSample<'a, 'a> {
        let pfg = self.core.reader_lock();
        let fresh_histogram = self
            .fresh_histogram_for_inactive()
            .expect("single-writer recorder inactive histogram settings must be reusable");
        let sample = self.perform_interval_sample_locked(Box::into_raw(Box::new(fresh_histogram)), &pfg);
        SingleWriterIntervalSample::new(self, sample, pfg)
    }

    fn fresh_histogram_for_inactive(&self) -> Result<Histogram, CreationError> {
        let mut fresh = Histogram::with_low_high_sigvdig(
            self.inactive_settings.lowest_discernible_value,
            self.inactive_settings.highest_trackable_value,
            self.inactive_settings.number_of_significant_value_digits as u8,
        )?;
        fresh.set_auto_resize(self.inactive_settings.auto_resize);
        fresh.set_integer_to_double_value_conversion_ratio(self.inactive_integer_to_double_value_conversion_ratio);
        Ok(fresh)
    }

    pub(in crate::concurrent) fn perform_interval_sample<'a>(
        &self,
        inactive_histogram: *mut Histogram,
        flip_guard: &PhaseFlipGuard<'a>,
    ) -> *mut Histogram {
        self.perform_interval_sample_locked(inactive_histogram, flip_guard)
    }

    fn perform_interval_sample_locked<'a>(&self, inactive_histogram: *mut Histogram, flip_guard: &PhaseFlipGuard<'a>) -> *mut Histogram {
        let _access = self.core.begin_sampling();
        let now = SystemTime::now();
        unsafe { (*inactive_histogram).meta_data.set_start_timestamp(now) };
        let active_histogram = self.core.swap_active(inactive_histogram);

        flip_guard.flip();

        unsafe { (*active_histogram).meta_data.set_end_timestamp(now) };
        active_histogram
    }
}

impl<P: OverflowPolicy> DoubleRecorderWithPolicy<P> {
    pub fn builder() -> DoubleRecorderBuilder<P> {
        DoubleRecorderBuilder::new()
    }

    pub fn from_histogram(mut histogram: ConcurrentDoubleHistogramWithPolicy<P>) -> Self {
        histogram.meta_data_mut().set_start_now();
        DoubleRecorderWithPolicy {
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
        let fresh_histogram = ConcurrentDoubleHistogramWithPolicy::<P>::with_highest_to_lowest_value_ratio(
            active.get_highest_to_lowest_value_ratio(),
            active.get_number_of_significant_value_digits(),
        )
        .expect("active double recorder histogram settings must be reusable");
        fresh_histogram.set_auto_resize(active.is_auto_resize());
        let sample = self.perform_interval_sample(Box::into_raw(Box::new(fresh_histogram)), &pfg);
        DoubleIntervalSample::new(self, sample, pfg)
    }

    pub(in crate::concurrent) fn perform_interval_sample<'a>(
        &self,
        inactive_histogram: *mut ConcurrentDoubleHistogramWithPolicy<P>,
        flip_guard: &PhaseFlipGuard<'a>,
    ) -> *mut ConcurrentDoubleHistogramWithPolicy<P> {
        let now = SystemTime::now();
        unsafe { (*inactive_histogram).meta_data_mut().set_start_timestamp(now) };
        let active_histogram = self.core.swap_active(inactive_histogram);
        flip_guard.flip();
        unsafe { (*active_histogram).meta_data_mut().set_end_timestamp(now) };
        active_histogram
    }
}

impl<P: OverflowPolicy> SingleWriterDoubleRecorderWithPolicy<P> {
    pub fn builder() -> SingleWriterDoubleRecorderBuilder<P> {
        SingleWriterDoubleRecorderBuilder::new()
    }

    pub fn from_histogram(mut histogram: DoubleHistogramWithPolicy<P>) -> Self {
        let inactive_highest_to_lowest_value_ratio = histogram.get_highest_to_lowest_value_ratio();
        let inactive_number_of_significant_value_digits = histogram.get_number_of_significant_value_digits();
        let inactive_auto_resize = histogram.is_auto_resize();
        histogram.meta_data_mut().set_start_now();
        SingleWriterDoubleRecorderWithPolicy {
            instance_id: get_instance_id(),
            core: SingleWriterCore::from_histogram(histogram),
            inactive_highest_to_lowest_value_ratio,
            inactive_number_of_significant_value_digits,
            inactive_auto_resize,
        }
    }

    #[inline(always)]
    pub fn record_value(&self, value: f64) -> Result<(), RecordError> {
        unsafe {
            let _access = self.core.begin_recording();
            (*self.core.active_after_recording_guard()).record_value(value)
        }
    }

    #[inline(always)]
    pub fn record_value_with_count(&self, value: f64, count: u64) -> Result<(), RecordError> {
        unsafe {
            let _access = self.core.begin_recording();
            (*self.core.active_after_recording_guard()).record_value_with_count(value, count)
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
            let _access = self.core.begin_recording();
            let active = &mut *self.core.active_after_recording_guard();
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
    /// The returned value owns the recorder's exclusive sampling lease. The
    /// single writer continues recording into the next interval after the swap;
    /// resampling may briefly wait for an in-flight writer call.
    pub fn begin_interval_sample<'a>(&'a self) -> SingleWriterDoubleIntervalSample<'a, 'a, P> {
        let pfg = self.core.reader_lock();
        let fresh_histogram = self
            .fresh_histogram_for_inactive()
            .expect("single-writer double recorder inactive histogram settings must be reusable");
        let sample = self.perform_interval_sample_locked(Box::into_raw(Box::new(fresh_histogram)), &pfg);
        SingleWriterDoubleIntervalSample::new(self, sample, pfg)
    }

    fn fresh_histogram_for_inactive(&self) -> Result<DoubleHistogramWithPolicy<P>, DoubleCreationError> {
        let mut fresh = DoubleHistogramWithPolicy::<P>::with_highest_to_lowest_value_ratio(
            self.inactive_highest_to_lowest_value_ratio,
            self.inactive_number_of_significant_value_digits,
        )?;
        fresh.set_auto_resize(self.inactive_auto_resize);
        Ok(fresh)
    }

    pub(in crate::concurrent) fn perform_interval_sample<'a>(
        &self,
        inactive_histogram: *mut DoubleHistogramWithPolicy<P>,
        flip_guard: &PhaseFlipGuard<'a>,
    ) -> *mut DoubleHistogramWithPolicy<P> {
        self.perform_interval_sample_locked(inactive_histogram, flip_guard)
    }

    fn perform_interval_sample_locked<'a>(
        &self,
        inactive_histogram: *mut DoubleHistogramWithPolicy<P>,
        flip_guard: &PhaseFlipGuard<'a>,
    ) -> *mut DoubleHistogramWithPolicy<P> {
        let _access = self.core.begin_sampling();
        let now = SystemTime::now();
        unsafe { (*inactive_histogram).meta_data_mut().set_start_timestamp(now) };
        let active_histogram = self.core.swap_active(inactive_histogram);
        flip_guard.flip();
        unsafe { (*active_histogram).meta_data_mut().set_end_timestamp(now) };
        active_histogram
    }
}
