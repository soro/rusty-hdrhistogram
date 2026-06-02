use crate::concurrent::double_histogram::{ConcurrentDoubleHistogram, ConcurrentDoubleHistogramImpl, SaturatingConcurrentDoubleHistogram};
use crate::concurrent::locking_sample::{
    DoubleLockingSample, LockingSample, ResizableLockingSample, SingleWriterDoubleLockingSample, SingleWriterLockingSample,
    StaticLockingSample,
};
use crate::concurrent::recordable_histogram::RecordableHistogram;
use crate::concurrent::resizable_histogram::ResizableConcurrentHistogram;
use crate::concurrent::static_histogram::StaticHistogram;
use crate::concurrent::writer_reader_phaser::{PhaseFlipGuard, WriterReaderPhaser};
use crate::core::*;
use crate::st::{DoubleHistogramImpl, Histogram};
use std::mem;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

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

pub struct StaticRecorder {
    inner: Recorder<StaticHistogram>,
}

pub struct ResizableRecorder {
    inner: Recorder<ResizableConcurrentHistogram>,
}

/// Recorder optimized for exactly one recording thread plus optional sampling.
///
/// Recording is kept as cheap as possible. Sampling waits for the current record
/// operation to finish and may be delayed indefinitely by a continuously active
/// writer; this is intentional for the single-writer variant.
pub struct SingleWriterRecorder {
    instance_id: usize,
    core: SingleWriterCore<Histogram<u64>>,
    inactive_settings: HistogramSettings,
    inactive_integer_to_double_value_conversion_ratio: f64,
}

pub type SaturatingDoubleRecorder = DoubleRecorder<SaturateOnOverflow>;
pub type SaturatingSingleWriterDoubleRecorder = SingleWriterDoubleRecorder<SaturateOnOverflow>;

pub struct DoubleRecorder<P: OverflowPolicy = ThrowOnOverflow> {
    instance_id: usize,
    core: RecorderCore<ConcurrentDoubleHistogramImpl<P>>,
}

/// Double recorder optimized for exactly one recording thread plus optional sampling.
///
/// Recording is kept as cheap as possible. Sampling waits for the current record
/// operation to finish and may be delayed indefinitely by a continuously active
/// writer; this is intentional for the single-writer variant.
pub struct SingleWriterDoubleRecorder<P: OverflowPolicy = ThrowOnOverflow> {
    instance_id: usize,
    core: SingleWriterCore<DoubleHistogramImpl<P>>,
    inactive_highest_to_lowest_value_ratio: u64,
    inactive_number_of_significant_value_digits: u8,
    inactive_auto_resize: bool,
}

pub fn static_with_low_high_sigvdig(
    lowest_discernible_value: u64,
    highest_trackable_value: u64,
    significant_value_digits: u8,
) -> Result<StaticRecorder, CreationError> {
    StaticHistogram::with_low_high_sigvdig(lowest_discernible_value, highest_trackable_value, significant_value_digits)
        .map(StaticRecorder::from_histogram)
}

pub fn resizable_with_low_high_sigvdig(
    lowest_discernible_value: u64,
    highest_trackable_value: u64,
    significant_value_digits: u8,
) -> Result<ResizableRecorder, CreationError> {
    ResizableConcurrentHistogram::with_low_high_sigvdig(lowest_discernible_value, highest_trackable_value, significant_value_digits)
        .map(ResizableRecorder::from_histogram)
}

pub fn single_writer(significant_value_digits: u8) -> Result<SingleWriterRecorder, CreationError> {
    SingleWriterRecorder::new(significant_value_digits)
}

pub fn single_writer_with_high_sigvdig(
    highest_trackable_value: u64,
    significant_value_digits: u8,
) -> Result<SingleWriterRecorder, CreationError> {
    SingleWriterRecorder::with_high_sigvdig(highest_trackable_value, significant_value_digits)
}

pub fn single_writer_with_low_high_sigvdig(
    lowest_discernible_value: u64,
    highest_trackable_value: u64,
    significant_value_digits: u8,
) -> Result<SingleWriterRecorder, CreationError> {
    SingleWriterRecorder::with_low_high_sigvdig(lowest_discernible_value, highest_trackable_value, significant_value_digits)
}

pub fn double(number_of_significant_value_digits: u8) -> Result<DoubleRecorder, DoubleCreationError> {
    ConcurrentDoubleHistogram::new(number_of_significant_value_digits).map(DoubleRecorder::from_histogram)
}

pub fn double_with_highest_to_lowest_value_ratio(
    highest_to_lowest_value_ratio: u64,
    number_of_significant_value_digits: u8,
) -> Result<DoubleRecorder, DoubleCreationError> {
    ConcurrentDoubleHistogram::with_highest_to_lowest_value_ratio(highest_to_lowest_value_ratio, number_of_significant_value_digits)
        .map(DoubleRecorder::from_histogram)
}

pub fn single_writer_double(number_of_significant_value_digits: u8) -> Result<SingleWriterDoubleRecorder, DoubleCreationError> {
    SingleWriterDoubleRecorder::new(number_of_significant_value_digits)
}

pub fn single_writer_double_with_highest_to_lowest_value_ratio(
    highest_to_lowest_value_ratio: u64,
    number_of_significant_value_digits: u8,
) -> Result<SingleWriterDoubleRecorder, DoubleCreationError> {
    SingleWriterDoubleRecorder::with_highest_to_lowest_value_ratio(highest_to_lowest_value_ratio, number_of_significant_value_digits)
}

pub fn saturating_double(number_of_significant_value_digits: u8) -> Result<SaturatingDoubleRecorder, DoubleCreationError> {
    SaturatingConcurrentDoubleHistogram::new(number_of_significant_value_digits).map(DoubleRecorder::from_histogram)
}

pub fn saturating_double_with_highest_to_lowest_value_ratio(
    highest_to_lowest_value_ratio: u64,
    number_of_significant_value_digits: u8,
) -> Result<SaturatingDoubleRecorder, DoubleCreationError> {
    SaturatingConcurrentDoubleHistogram::with_highest_to_lowest_value_ratio(
        highest_to_lowest_value_ratio,
        number_of_significant_value_digits,
    )
    .map(DoubleRecorder::from_histogram)
}

pub fn saturating_single_writer_double(
    number_of_significant_value_digits: u8,
) -> Result<SaturatingSingleWriterDoubleRecorder, DoubleCreationError> {
    SaturatingSingleWriterDoubleRecorder::new(number_of_significant_value_digits)
}

pub fn saturating_single_writer_double_with_highest_to_lowest_value_ratio(
    highest_to_lowest_value_ratio: u64,
    number_of_significant_value_digits: u8,
) -> Result<SaturatingSingleWriterDoubleRecorder, DoubleCreationError> {
    SaturatingSingleWriterDoubleRecorder::with_highest_to_lowest_value_ratio(
        highest_to_lowest_value_ratio,
        number_of_significant_value_digits,
    )
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
    pub(crate) fn from_histogram(histogram: T) -> Recorder<T> {
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

    pub(crate) fn locking_sample<'a>(&'a self) -> LockingSample<'a, 'a, T> {
        let pfg = self.core.reader_lock();
        let settings = unsafe { (&*self.core.active()).settings() };
        let fresh_histogram = Box::new(T::fresh(&settings).unwrap());
        let sample = self.perform_interval_sample(Box::into_raw(fresh_histogram), &pfg);
        LockingSample::new(self, sample, pfg)
    }

    pub(in crate::concurrent) fn perform_interval_sample<'a>(&self, inactive_histogram: *mut T, flip_guard: &PhaseFlipGuard<'a>) -> *mut T {
        let active_histogram = self.core.swap_active(inactive_histogram);
        unsafe { (*inactive_histogram).meta_data_mut().set_start_now() };

        flip_guard.flip();

        unsafe { (*active_histogram).meta_data_mut().set_end_now() };
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

            pub fn locking_sample<'a>(&'a self) -> $sample<'a, 'a> {
                $sample::new(self.inner.locking_sample())
            }
        }
    };
}

impl_integer_recorder!(StaticRecorder, StaticHistogram, StaticLockingSample);
impl_integer_recorder!(ResizableRecorder, ResizableConcurrentHistogram, ResizableLockingSample);

impl SingleWriterRecorder {
    pub fn new(significant_value_digits: u8) -> Result<Self, CreationError> {
        let mut histogram = Histogram::<u64>::new(significant_value_digits)?;
        histogram.set_auto_resize(true);
        Ok(Self::from_histogram(histogram))
    }

    pub fn with_high_sigvdig(highest_trackable_value: u64, significant_value_digits: u8) -> Result<Self, CreationError> {
        Histogram::<u64>::with_high_sigvdig(highest_trackable_value, significant_value_digits).map(Self::from_histogram)
    }

    pub fn with_low_high_sigvdig(
        lowest_discernible_value: u64,
        highest_trackable_value: u64,
        significant_value_digits: u8,
    ) -> Result<Self, CreationError> {
        Histogram::<u64>::with_low_high_sigvdig(lowest_discernible_value, highest_trackable_value, significant_value_digits)
            .map(Self::from_histogram)
    }

    pub fn from_histogram(histogram: Histogram<u64>) -> Self {
        let inactive_settings = histogram.settings().clone();
        let inactive_integer_to_double_value_conversion_ratio = histogram.integer_to_double_value_conversion_ratio();
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

    pub fn locking_sample<'a>(&'a self) -> SingleWriterLockingSample<'a, 'a> {
        let pfg = self.core.reader_lock();
        let fresh_histogram = self
            .fresh_histogram_for_inactive()
            .expect("single-writer recorder inactive histogram settings must be reusable");
        let sample = self.perform_interval_sample_locked(Box::into_raw(Box::new(fresh_histogram)), &pfg);
        SingleWriterLockingSample::new(self, sample, pfg)
    }

    fn fresh_histogram_for_inactive(&self) -> Result<Histogram<u64>, CreationError> {
        let mut fresh = Histogram::<u64>::with_low_high_sigvdig(
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
        inactive_histogram: *mut Histogram<u64>,
        flip_guard: &PhaseFlipGuard<'a>,
    ) -> *mut Histogram<u64> {
        self.perform_interval_sample_locked(inactive_histogram, flip_guard)
    }

    fn perform_interval_sample_locked<'a>(
        &self,
        inactive_histogram: *mut Histogram<u64>,
        flip_guard: &PhaseFlipGuard<'a>,
    ) -> *mut Histogram<u64> {
        let _access = self.core.begin_sampling();
        let active_histogram = self.core.swap_active(inactive_histogram);
        unsafe { (*inactive_histogram).meta_data.set_start_now() };

        flip_guard.flip();

        unsafe { (*active_histogram).meta_data.set_end_now() };
        active_histogram
    }
}

impl<P: OverflowPolicy> DoubleRecorder<P> {
    pub fn from_histogram(histogram: ConcurrentDoubleHistogramImpl<P>) -> Self {
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

    pub fn locking_sample<'a>(&'a self) -> DoubleLockingSample<'a, 'a, P> {
        let pfg = self.core.reader_lock();
        let active = unsafe { &*self.core.active() };
        let fresh_histogram = ConcurrentDoubleHistogramImpl::<P>::with_highest_to_lowest_value_ratio(
            active.get_highest_to_lowest_value_ratio(),
            active.get_number_of_significant_value_digits(),
        )
        .expect("active double recorder histogram settings must be reusable");
        fresh_histogram.set_auto_resize(active.is_auto_resize());
        let sample = self.perform_interval_sample(Box::into_raw(Box::new(fresh_histogram)), &pfg);
        DoubleLockingSample::new(self, sample, pfg)
    }

    pub(in crate::concurrent) fn perform_interval_sample<'a>(
        &self,
        inactive_histogram: *mut ConcurrentDoubleHistogramImpl<P>,
        flip_guard: &PhaseFlipGuard<'a>,
    ) -> *mut ConcurrentDoubleHistogramImpl<P> {
        let active_histogram = self.core.swap_active(inactive_histogram);
        flip_guard.flip();
        active_histogram
    }
}

impl<P: OverflowPolicy> SingleWriterDoubleRecorder<P> {
    pub fn new(number_of_significant_value_digits: u8) -> Result<Self, DoubleCreationError> {
        DoubleHistogramImpl::<P>::new(number_of_significant_value_digits).map(Self::from_histogram)
    }

    pub fn with_highest_to_lowest_value_ratio(
        highest_to_lowest_value_ratio: u64,
        number_of_significant_value_digits: u8,
    ) -> Result<Self, DoubleCreationError> {
        DoubleHistogramImpl::<P>::with_highest_to_lowest_value_ratio(highest_to_lowest_value_ratio, number_of_significant_value_digits)
            .map(Self::from_histogram)
    }

    pub fn from_histogram(histogram: DoubleHistogramImpl<P>) -> Self {
        let inactive_highest_to_lowest_value_ratio = histogram.get_highest_to_lowest_value_ratio();
        let inactive_number_of_significant_value_digits = histogram.get_number_of_significant_value_digits();
        let inactive_auto_resize = histogram.is_auto_resize();
        SingleWriterDoubleRecorder {
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

    pub fn locking_sample<'a>(&'a self) -> SingleWriterDoubleLockingSample<'a, 'a, P> {
        let pfg = self.core.reader_lock();
        let fresh_histogram = self
            .fresh_histogram_for_inactive()
            .expect("single-writer double recorder inactive histogram settings must be reusable");
        let sample = self.perform_interval_sample_locked(Box::into_raw(Box::new(fresh_histogram)), &pfg);
        SingleWriterDoubleLockingSample::new(self, sample, pfg)
    }

    fn fresh_histogram_for_inactive(&self) -> Result<DoubleHistogramImpl<P>, DoubleCreationError> {
        let mut fresh = DoubleHistogramImpl::<P>::with_highest_to_lowest_value_ratio(
            self.inactive_highest_to_lowest_value_ratio,
            self.inactive_number_of_significant_value_digits,
        )?;
        fresh.set_auto_resize(self.inactive_auto_resize);
        Ok(fresh)
    }

    pub(in crate::concurrent) fn perform_interval_sample<'a>(
        &self,
        inactive_histogram: *mut DoubleHistogramImpl<P>,
        flip_guard: &PhaseFlipGuard<'a>,
    ) -> *mut DoubleHistogramImpl<P> {
        self.perform_interval_sample_locked(inactive_histogram, flip_guard)
    }

    fn perform_interval_sample_locked<'a>(
        &self,
        inactive_histogram: *mut DoubleHistogramImpl<P>,
        flip_guard: &PhaseFlipGuard<'a>,
    ) -> *mut DoubleHistogramImpl<P> {
        let _access = self.core.begin_sampling();
        let active_histogram = self.core.swap_active(inactive_histogram);
        flip_guard.flip();
        active_histogram
    }
}
