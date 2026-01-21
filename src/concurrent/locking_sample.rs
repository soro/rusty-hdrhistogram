use crate::concurrent::recordable_histogram::RecordableHistogram;
use crate::concurrent::recorder::Recorder;
use crate::concurrent::snapshot::Snapshot;
use crate::concurrent::writer_reader_phaser::PhaseFlipGuard;
use std::mem;
use std::sync::atomic::{AtomicPtr, Ordering};

pub struct LockingSample<'a, 'b: 'a, T: 'a + RecordableHistogram> {
    parent_recorder: &'a Recorder<T>,
    histogram: AtomicPtr<T>,
    guard: PhaseFlipGuard<'b>,
}

impl<'a, 'b: 'a, T: RecordableHistogram> LockingSample<'a, 'b, T> {
    pub fn new(parent_recorder: &'a Recorder<T>, histogram: *mut T, guard: PhaseFlipGuard<'b>) -> Self {
        let sample = LockingSample {
            parent_recorder,
            histogram: AtomicPtr::new(histogram),
            guard,
        };
        sample
    }

    pub fn resample(self) -> Self {
        unsafe {
            let to_swap = self.histogram.load(Ordering::Acquire);
            (*to_swap).clear_counts();
            let res = self.parent_recorder
                .perform_interval_sample(to_swap, &self.guard);
            self.histogram.store(res, Ordering::Release);
            self
        }
    }

    pub fn histogram(&self) -> Snapshot<'_, T> {
        unsafe { Snapshot::new(&mut *self.histogram.load(Ordering::Relaxed)) }
    }
}

impl<'a, 'b: 'a, T: RecordableHistogram> Drop for LockingSample<'a, 'b, T> {
    fn drop(&mut self) {
        unsafe {
            self.guard.flip();
            mem::drop(Box::from_raw(self.histogram.load(Ordering::SeqCst)));
        }
    }
}
