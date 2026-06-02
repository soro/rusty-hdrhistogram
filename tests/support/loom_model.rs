#![allow(dead_code)]

use loom::sync::atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering};
use loom::sync::{Arc, Mutex, MutexGuard};
use loom::thread;
use std::mem;

pub struct ModelPhaser {
    start_epoch: AtomicIsize,
    even_end_epoch: AtomicIsize,
    odd_end_epoch: AtomicIsize,
    reader_lock: Mutex<()>,
}

impl ModelPhaser {
    pub fn new() -> Self {
        Self {
            start_epoch: AtomicIsize::new(0),
            even_end_epoch: AtomicIsize::new(0),
            odd_end_epoch: AtomicIsize::new(isize::MIN),
            reader_lock: Mutex::new(()),
        }
    }

    pub fn begin_writer_critical_section(&self) -> WriterCriticalSectionGuard<'_> {
        let critical_value = self.start_epoch.fetch_add(1, Ordering::Acquire);
        let epoch = if critical_value < 0 {
            &self.odd_end_epoch
        } else {
            &self.even_end_epoch
        };
        WriterCriticalSectionGuard { epoch }
    }

    pub fn reader_lock(&self) -> PhaseFlipGuard<'_> {
        let guard = self.reader_lock.lock().unwrap();
        PhaseFlipGuard {
            parent: self,
            _guard: guard,
        }
    }
}

pub struct WriterCriticalSectionGuard<'a> {
    epoch: &'a AtomicIsize,
}

impl Drop for WriterCriticalSectionGuard<'_> {
    fn drop(&mut self) {
        self.epoch.fetch_add(1, Ordering::Release);
    }
}

pub struct PhaseFlipGuard<'a> {
    parent: &'a ModelPhaser,
    _guard: MutexGuard<'a, ()>,
}

impl PhaseFlipGuard<'_> {
    pub fn flip(&self) {
        // Keep this ordering mirror aligned with src/concurrent/writer_reader_phaser.rs.
        let next_phase_is_even = self.parent.start_epoch.load(Ordering::SeqCst) < 0;
        let initial_start_value = if next_phase_is_even { 0 } else { isize::MIN };

        if next_phase_is_even {
            self.parent.even_end_epoch.store(initial_start_value, Ordering::Relaxed);
        } else {
            self.parent.odd_end_epoch.store(initial_start_value, Ordering::Relaxed);
        }

        let start_value_at_flip = self.parent.start_epoch.swap(initial_start_value, Ordering::SeqCst);

        loop {
            let caught_up = if next_phase_is_even {
                self.parent.odd_end_epoch.load(Ordering::Acquire) == start_value_at_flip
            } else {
                self.parent.even_end_epoch.load(Ordering::Acquire) == start_value_at_flip
            };

            if caught_up {
                break;
            }
            thread::yield_now();
        }
    }
}

pub struct ModelRecorder {
    phaser: ModelPhaser,
    active: AtomicUsize,
    counts: [AtomicUsize; 2],
}

impl ModelRecorder {
    pub fn new() -> Self {
        Self {
            phaser: ModelPhaser::new(),
            active: AtomicUsize::new(0),
            counts: [AtomicUsize::new(0), AtomicUsize::new(0)],
        }
    }

    pub fn record_one(&self) {
        let _guard = self.phaser.begin_writer_critical_section();
        thread::yield_now();
        let active = self.active.load(Ordering::Acquire);
        thread::yield_now();
        self.counts[active].fetch_add(1, Ordering::Relaxed);
    }

    pub fn resample_into(&self, inactive: usize) -> usize {
        let guard = self.phaser.reader_lock();
        self.counts[inactive].store(0, Ordering::Relaxed);
        let sampled = self.active.swap(inactive, Ordering::SeqCst);
        guard.flip();
        sampled
    }

    pub fn wait_until_active(&self, expected: usize) {
        while self.active.load(Ordering::Acquire) != expected {
            thread::yield_now();
        }
    }

    pub fn count(&self, histogram: usize) -> usize {
        self.counts[histogram].load(Ordering::Relaxed)
    }

    pub fn total_count(&self) -> usize {
        self.count(0) + self.count(1)
    }

    pub fn set_count(&self, histogram: usize, count: usize) {
        self.counts[histogram].store(count, Ordering::Relaxed);
    }
}

pub struct ModelResizableConcurrentHistogram {
    phaser: ModelPhaser,
    active: AtomicUsize,
    inactive: AtomicUsize,
    lengths: [AtomicUsize; 4],
    retired: [AtomicBool; 4],
    counts: [AtomicUsize; 4],
}

impl ModelResizableConcurrentHistogram {
    pub fn new() -> Self {
        Self {
            phaser: ModelPhaser::new(),
            active: AtomicUsize::new(0),
            inactive: AtomicUsize::new(1),
            lengths: [AtomicUsize::new(1), AtomicUsize::new(1), AtomicUsize::new(2), AtomicUsize::new(2)],
            retired: [
                AtomicBool::new(false),
                AtomicBool::new(false),
                AtomicBool::new(false),
                AtomicBool::new(false),
            ],
            counts: [AtomicUsize::new(1), AtomicUsize::new(10), AtomicUsize::new(0), AtomicUsize::new(0)],
        }
    }

    pub fn read_view(&self) -> ModelReadView<'_> {
        let guard = self.phaser.reader_lock();
        let active = self.active.load(Ordering::Acquire);
        thread::yield_now();
        let inactive = self.inactive.load(Ordering::Relaxed);
        let active_len = self.length(active);
        let inactive_len = self.length(inactive);
        assert_eq!(active_len, inactive_len);
        ModelReadView {
            histogram: self,
            _guard: guard,
            active,
            inactive,
            active_len,
            inactive_len,
        }
    }

    pub fn record_one(&self) {
        let _guard = self.phaser.begin_writer_critical_section();
        let active = self.active.load(Ordering::Acquire);
        self.assert_not_retired(active);
        thread::yield_now();
        self.counts[active].fetch_add(1, Ordering::Relaxed);
    }

    pub fn resize_to_second_pair(&self) {
        let guard = self.phaser.reader_lock();

        let previous_inactive = self.inactive.load(Ordering::Relaxed);
        self.assert_not_retired(previous_inactive);
        self.counts[2].store(self.count(previous_inactive), Ordering::Relaxed);
        self.inactive.store(2, Ordering::SeqCst);
        self.swap_active_inactive();
        guard.flip();

        let previous_active = self.inactive.load(Ordering::Relaxed);
        self.assert_not_retired(previous_active);
        self.counts[3].store(self.count(previous_active), Ordering::Relaxed);
        self.inactive.store(3, Ordering::SeqCst);
        self.swap_active_inactive();
        guard.flip();

        self.retired[previous_active].store(true, Ordering::Release);
        self.retired[previous_inactive].store(true, Ordering::Release);
    }

    pub fn active_pair_is_second_pair(&self) -> bool {
        self.active.load(Ordering::Acquire) == 3 && self.inactive.load(Ordering::Acquire) == 2
    }

    pub fn old_pair_is_retired(&self) -> bool {
        self.is_retired(0) && self.is_retired(1)
    }

    pub fn total_count_all(&self) -> usize {
        self.count(0) + self.count(1) + self.count(2) + self.count(3)
    }

    fn swap_active_inactive(&self) {
        let active = self.active.load(Ordering::Acquire);
        let inactive = self.inactive.load(Ordering::Relaxed);
        self.active.store(inactive, Ordering::SeqCst);
        self.inactive.store(active, Ordering::SeqCst);
    }

    fn count(&self, backing: usize) -> usize {
        self.counts[backing].load(Ordering::Relaxed)
    }

    fn length(&self, backing: usize) -> usize {
        self.lengths[backing].load(Ordering::Relaxed)
    }

    fn is_retired(&self, backing: usize) -> bool {
        self.retired[backing].load(Ordering::Acquire)
    }

    fn assert_not_retired(&self, backing: usize) {
        assert!(!self.is_retired(backing));
    }
}

pub struct ModelReadView<'a> {
    histogram: &'a ModelResizableConcurrentHistogram,
    _guard: PhaseFlipGuard<'a>,
    active: usize,
    inactive: usize,
    active_len: usize,
    inactive_len: usize,
}

impl ModelReadView<'_> {
    pub fn assert_captured_pair_is_live_and_consistent(&self) {
        self.histogram.assert_not_retired(self.active);
        self.histogram.assert_not_retired(self.inactive);
        assert_eq!(self.active_len, self.histogram.length(self.active));
        assert_eq!(self.inactive_len, self.histogram.length(self.inactive));
        assert_eq!(self.active_len, self.inactive_len);
    }

    pub fn total_count(&self) -> usize {
        self.assert_captured_pair_is_live_and_consistent();
        self.histogram.count(self.active) + self.histogram.count(self.inactive)
    }
}

pub struct ModelConcurrentDoubleGate {
    phaser: ModelPhaser,
    gate_closed: AtomicBool,
    mutating: AtomicBool,
    count: AtomicUsize,
    total_count: AtomicUsize,
    lowest: AtomicUsize,
    ratio: AtomicUsize,
    range_generation: AtomicUsize,
}

impl ModelConcurrentDoubleGate {
    pub fn new() -> Self {
        Self {
            phaser: ModelPhaser::new(),
            gate_closed: AtomicBool::new(false),
            mutating: AtomicBool::new(false),
            count: AtomicUsize::new(0),
            total_count: AtomicUsize::new(0),
            lowest: AtomicUsize::new(1),
            ratio: AtomicUsize::new(1),
            range_generation: AtomicUsize::new(0),
        }
    }

    pub fn read_view(&self) -> ModelDoubleReadView<'_> {
        ModelDoubleReadView {
            _guard: self.phaser.reader_lock(),
        }
    }

    pub fn try_record_one(&self) -> bool {
        let _guard = self.phaser.begin_writer_critical_section();
        thread::yield_now();
        if self.gate_closed.load(Ordering::SeqCst) {
            return false;
        }
        thread::yield_now();
        assert!(!self.mutating.load(Ordering::Acquire));
        self.count.fetch_add(1, Ordering::Relaxed);
        true
    }

    pub fn try_record_one_with_total(&self) -> bool {
        let _guard = self.phaser.begin_writer_critical_section();
        thread::yield_now();
        if self.gate_closed.load(Ordering::SeqCst) {
            return false;
        }
        self.count.fetch_add(1, Ordering::Relaxed);
        thread::yield_now();
        self.total_count.fetch_add(1, Ordering::Relaxed);
        true
    }

    pub fn try_record_value_after_range_decision(&self, value: usize) -> bool {
        let observed_generation = self.range_generation.load(Ordering::Acquire);
        let current_lowest = self.lowest.load(Ordering::Relaxed);
        thread::yield_now();
        if value < current_lowest {
            return false;
        }

        let _guard = self.phaser.begin_writer_critical_section();
        thread::yield_now();
        if self.gate_closed.load(Ordering::SeqCst) {
            return false;
        }
        if self.range_generation.load(Ordering::Acquire) != observed_generation {
            return false;
        }

        assert!(
            value >= self.lowest.load(Ordering::Relaxed),
            "recorded a value using a stale pre-gate range decision"
        );
        self.count.fetch_add(1, Ordering::Relaxed);
        true
    }

    pub fn begin_structural_mutation(&self) -> ModelDoubleStructuralMutation<'_> {
        ModelDoubleStructuralMutation {
            histogram: self,
            guard: self.phaser.reader_lock(),
        }
    }

    pub fn shift_after_locking_phaser(&self) {
        let mut mutation = self.begin_structural_mutation();
        thread::yield_now();
        self.gate_closed.store(true, Ordering::SeqCst);
        mutation.flip();
        mutation.publish_ratio(2);
        self.lowest.store(2, Ordering::Relaxed);
        self.range_generation.fetch_add(1, Ordering::Release);
        self.gate_closed.store(false, Ordering::SeqCst);
    }

    pub fn reset_after_locking_phaser(&self) {
        let guard = self.phaser.reader_lock();
        self.gate_closed.store(true, Ordering::SeqCst);
        guard.flip();
        self.count.store(0, Ordering::Relaxed);
        thread::yield_now();
        self.total_count.store(0, Ordering::Relaxed);
        self.lowest.store(1, Ordering::Relaxed);
        self.range_generation.fetch_add(1, Ordering::Release);
        self.gate_closed.store(false, Ordering::SeqCst);
    }

    pub fn count(&self) -> usize {
        self.count.load(Ordering::Relaxed)
    }

    pub fn count_equals_total(&self) -> bool {
        self.count.load(Ordering::Relaxed) == self.total_count.load(Ordering::Relaxed)
    }

    pub fn ratio(&self) -> usize {
        self.ratio.load(Ordering::Relaxed)
    }
}

pub struct ModelDoubleReadView<'a> {
    _guard: PhaseFlipGuard<'a>,
}

pub struct ModelDoubleStructuralMutation<'a> {
    histogram: &'a ModelConcurrentDoubleGate,
    guard: PhaseFlipGuard<'a>,
}

impl ModelDoubleStructuralMutation<'_> {
    pub fn flip(&mut self) {
        self.guard.flip();
    }

    pub fn publish_ratio(&mut self, ratio: usize) {
        self.histogram.mutating.store(true, Ordering::Release);
        thread::yield_now();
        self.guard.flip();
        thread::yield_now();
        self.guard.flip();
        self.histogram.ratio.store(ratio, Ordering::Relaxed);
        self.histogram.mutating.store(false, Ordering::Release);
    }
}

pub fn run_model<F>(f: F)
where
    F: Fn() + Sync + Send + 'static,
{
    loom::model(f);
}

pub fn spawn<F>(f: F) -> loom::thread::JoinHandle<()>
where
    F: FnOnce() + Send + 'static,
{
    loom::thread::spawn(f)
}

pub fn arc<T>(value: T) -> Arc<T> {
    Arc::new(value)
}

pub fn atomic_bool(value: bool) -> AtomicBool {
    AtomicBool::new(value)
}

pub fn atomic_usize(value: usize) -> AtomicUsize {
    AtomicUsize::new(value)
}

pub fn drop_guard<T>(guard: T) {
    mem::drop(guard);
}
