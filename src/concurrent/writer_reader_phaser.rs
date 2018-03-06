//! ReaderWriterPhaser implementation used in concurrent Histograms
//!
//! Writers acquire a write interface by invoking
//! ```let wi = WriterReaderPhaser::write(phaser)```
//! and then call
//! ```let section_guard = wi.begin_critical_write_section()```
//! before performing an update of the data managed by the phaser.
//! Readers have to acquire a read interface via
//! ```let ri = WriterReaderPhaser.read(phaser)```
//! and can then lock and read the data by calling
//! ```let rg = ri.reader_lock()```
//! before finally calling `rg.flip()` once they are done executing the swap.

use parking_lot::Mutex;
use std::isize::MIN as ISIZE_MIN;
use std::mem;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::thread;
use std::time::Duration;

// Struct holding all the bookkeeping variables for the phaser
pub struct WriterReaderPhaser {
    start_epoch: AtomicIsize,
    even_end_epoch: AtomicIsize,
    odd_end_epoch: AtomicIsize,
    reader_lock: Mutex<()>,
}

impl WriterReaderPhaser {
    pub fn new() -> WriterReaderPhaser {
        let start = AtomicIsize::new(0);
        let even_end = AtomicIsize::new(0);
        let odd_end = AtomicIsize::new(ISIZE_MIN);

        WriterReaderPhaser {
            start_epoch: start,
            even_end_epoch: even_end,
            odd_end_epoch: odd_end,
            reader_lock: Mutex::new(()),
        }
    }

    pub fn begin_writer_critical_section<'a>(&'a self) -> WriterCriticalSectionGuard<'a> {
        let critical_value = self.start_epoch.fetch_add(1, Ordering::Acquire);
        if critical_value < 0 {
            WriterCriticalSectionGuard {
                epoch: &self.odd_end_epoch,
            }
        } else {
            WriterCriticalSectionGuard {
                epoch: &self.even_end_epoch,
            }
        }
    }

    pub fn reader_lock<'a>(&'a self) -> PhaseFlipGuard<'a> {
        self.reader_lock.raw_lock();
        PhaseFlipGuard { parent: &self }
    }
}

pub struct WriterCriticalSectionGuard<'a> {
    epoch: &'a AtomicIsize,
}

impl<'a> WriterCriticalSectionGuard<'a> {
    pub fn end_writer_critical_section(self) {
        mem::drop(self);
    }
}

impl<'a> Drop for WriterCriticalSectionGuard<'a> {
    #[allow(unused_results)]
    fn drop(&mut self) {
        self.epoch.fetch_add(1, Ordering::Release);
    }
}

// Guard used to enforce lock before flip
pub struct PhaseFlipGuard<'a> {
    parent: &'a WriterReaderPhaser,
}

impl<'a> PhaseFlipGuard<'a> {
    pub fn flip_with_yield_time(&self, yield_time: Duration) {
        let next_phase_is_even = self.parent.start_epoch.load(Ordering::SeqCst) < 0;

        let initial_start_value = if next_phase_is_even { 0 } else { ISIZE_MIN };
        if next_phase_is_even {
            self.parent
                .even_end_epoch
                .store(initial_start_value, Ordering::Relaxed);
        } else {
            self.parent
                .odd_end_epoch
                .store(initial_start_value, Ordering::Relaxed);
        }

        let start_value_at_flip = self.parent
            .start_epoch
            .swap(initial_start_value, Ordering::SeqCst);

        loop {
            let caught_up = if next_phase_is_even {
                self.parent.odd_end_epoch.load(Ordering::Relaxed) == start_value_at_flip
            } else {
                self.parent.even_end_epoch.load(Ordering::Relaxed) == start_value_at_flip
            };
            if !caught_up {
                if yield_time.as_secs() == 0 && yield_time.subsec_nanos() == 0 {
                    thread::yield_now();
                } else {
                    thread::sleep(yield_time);
                }
            } else {
                break;
            }
        }
    }

    pub fn flip(&self) {
        self.flip_with_yield_time(Duration::new(0, 0))
    }

    pub fn reader_unlock(self) {
        mem::drop(self)
    }
}

impl<'a> Drop for PhaseFlipGuard<'a> {
    fn drop(&mut self) {
        unsafe { self.parent.reader_lock.raw_unlock() }
    }
}
