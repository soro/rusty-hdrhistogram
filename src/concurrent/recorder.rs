use crate::concurrent::locking_sample::LockingSample;
use crate::concurrent::recordable_histogram::RecordableHistogram;
use crate::concurrent::resizable_histogram::ResizableHistogram;
use crate::concurrent::static_histogram::StaticHistogram;
use crate::concurrent::writer_reader_phaser::{PhaseFlipGuard, WriterReaderPhaser};
use crate::core::*;
use std::mem;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

static REPORTER_INSTANCE_SEQUENCER: AtomicUsize = AtomicUsize::new(0);
fn get_instance_id() -> usize {
    REPORTER_INSTANCE_SEQUENCER.fetch_add(1, Ordering::Relaxed)
}

pub struct Recorder<T: RecordableHistogram> {
    pub instance_id: usize,
    recording_phaser: WriterReaderPhaser,
    active_histogram: AtomicPtr<T>,
}

pub type StaticRecorder<const N: usize> = Recorder<StaticHistogram<N>>;
pub type ResizableRecorder = Recorder<ResizableHistogram>;

pub fn static_with_low_high_sigvdig<const N: usize>(
    lowest_discernible_value: u64,
    highest_trackable_value: u64,
    significant_value_digits: u8,
) -> Result<StaticRecorder<N>, CreationError> {
    StaticHistogram::<N>::with_low_high_sigvdig(
        lowest_discernible_value,
        highest_trackable_value,
        significant_value_digits,
    ).map(Recorder::from_histogram)
}

pub fn resizable_with_low_high_sigvdig(
    lowest_discernible_value: u64,
    highest_trackable_value: u64,
    significant_value_digits: u8,
) -> Result<ResizableRecorder, CreationError> {
    ResizableHistogram::with_low_high_sigvdig(
        lowest_discernible_value,
        highest_trackable_value,
        significant_value_digits,
    ).map(Recorder::from_histogram)
}

impl<T: RecordableHistogram> Recorder<T> {
    pub fn from_histogram(histogram: T) -> Recorder<T> {
        let boxed_histo = Box::new(histogram);
        Recorder {
            instance_id: get_instance_id(),
            recording_phaser: WriterReaderPhaser::new(),
            active_histogram: AtomicPtr::new(Box::into_raw(boxed_histo)),
        }
    }
    pub fn record_value(&self, value: u64) -> Result<(), RecordError> {
        unsafe {
            let _csg = self.recording_phaser.begin_writer_critical_section();
            (*self.active_histogram.load(Ordering::Relaxed)).record_value(value)
        }
    }

    pub fn record_value_with_count(&self, value: u64, count: u64) -> Result<(), RecordError> {
        unsafe {
            let _csg = self.recording_phaser.begin_writer_critical_section();
            (*self.active_histogram.load(Ordering::Relaxed)).record_value_with_count(value, count)
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
            let _csg = self.recording_phaser.begin_writer_critical_section();
            let active_histogram = &*self.active_histogram.load(Ordering::Relaxed);
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
    pub fn record_value_with_expected_interval(&self, value: u64, expected_interval_betwee_values: u64) -> Result<(), RecordError> {
        self.record_value_with_count_and_expected_interval(value, 1, expected_interval_betwee_values)
    }

    pub fn locking_sample(&self) -> LockingSample<T> {
        let pfg = self.recording_phaser.reader_lock();
        let settings = unsafe { (&*self.active_histogram.load(Ordering::Relaxed)).settings() };
        let fresh_histogram = Box::new(T::fresh(settings).unwrap());
        let sample = self.perform_interval_sample(Box::into_raw(fresh_histogram) as *mut T, &pfg);
        LockingSample::new(&self, sample, pfg)
    }

    pub(in concurrent) fn perform_interval_sample<'a>(&self, inactive_histogram: *mut T, flip_guard: &PhaseFlipGuard<'a>) -> *mut T {
        let active_histogram = self.active_histogram
            .swap(inactive_histogram, Ordering::SeqCst);
        unsafe {
            (*self.active_histogram.load(Ordering::Relaxed))
                .meta_data_mut()
                .set_start_now()
        };

        flip_guard.flip();

        unsafe { (*active_histogram).meta_data_mut().set_end_now() };
        active_histogram
    }
}

impl<T: RecordableHistogram> Drop for Recorder<T> {
    fn drop(&mut self) {
        unsafe {
            self.recording_phaser.reader_lock().flip();
            mem::drop(Box::from_raw(self.active_histogram.load(Ordering::SeqCst)));
        }
    }
}
