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
//!
//! Unlike Java's `WriterReaderPhaser`, this reader lock is not reentrant. Do not
//! acquire another reader lock on the same phaser while holding a `PhaseFlipGuard`.

use parking_lot::{Mutex, MutexGuard};
use std::mem;
use std::sync::atomic::{AtomicI64, Ordering};
use std::thread;
use std::time::Duration;

// Struct holding all the bookkeeping variables for the phaser
pub struct WriterReaderPhaser {
    // The sign identifies the phase. Keep Java's 64-bit epochs even on 32-bit
    // targets so ordinary recording cannot reach a phase rollover at 2^31 writes.
    start_epoch: AtomicI64,
    even_end_epoch: AtomicI64,
    odd_end_epoch: AtomicI64,
    reader_lock: Mutex<()>,
}

impl WriterReaderPhaser {
    pub fn new() -> WriterReaderPhaser {
        let start = AtomicI64::new(0);
        let even_end = AtomicI64::new(0);
        let odd_end = AtomicI64::new(i64::MIN);

        WriterReaderPhaser {
            start_epoch: start,
            even_end_epoch: even_end,
            odd_end_epoch: odd_end,
            reader_lock: Mutex::new(()),
        }
    }

    #[inline(always)]
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
        let guard = self.reader_lock.lock();
        PhaseFlipGuard {
            parent: self,
            _guard: guard,
        }
    }
}

pub struct WriterCriticalSectionGuard<'a> {
    epoch: &'a AtomicI64,
}

impl<'a> WriterCriticalSectionGuard<'a> {
    pub fn end_writer_critical_section(self) {
        mem::drop(self);
    }
}

impl<'a> Drop for WriterCriticalSectionGuard<'a> {
    #[allow(unused_results)]
    #[inline(always)]
    fn drop(&mut self) {
        self.epoch.fetch_add(1, Ordering::Release);
    }
}

// Guard used to enforce lock before flip
pub struct PhaseFlipGuard<'a> {
    parent: &'a WriterReaderPhaser,
    _guard: MutexGuard<'a, ()>,
}

impl<'a> PhaseFlipGuard<'a> {
    pub fn flip_with_yield_time(&self, yield_time: Duration) {
        let next_phase_is_even = self.parent.start_epoch.load(Ordering::SeqCst) < 0;

        let initial_start_value = if next_phase_is_even { 0 } else { i64::MIN };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epochs_keep_their_phase_past_the_32_bit_boundary() {
        for odd_phase in [false, true] {
            let initial_epoch = if odd_phase { i64::MIN } else { 0 };
            let completed_epoch = initial_epoch + i64::from(i32::MAX);
            // Seed completed entries instead of recording billions of writes.
            let phaser = WriterReaderPhaser {
                start_epoch: AtomicI64::new(completed_epoch),
                even_end_epoch: AtomicI64::new(if odd_phase { 0 } else { completed_epoch }),
                odd_end_epoch: AtomicI64::new(if odd_phase { completed_epoch } else { i64::MIN }),
                reader_lock: Mutex::new(()),
            };
            let end_epoch = if odd_phase { &phaser.odd_end_epoch } else { &phaser.even_end_epoch };
            let writer = phaser.begin_writer_critical_section();
            assert!(std::ptr::eq(writer.epoch, end_epoch));
            assert_eq!(completed_epoch + 1, phaser.start_epoch.load(Ordering::Relaxed));
            assert_eq!(odd_phase, phaser.start_epoch.load(Ordering::Relaxed) < 0);
            assert_eq!(completed_epoch, end_epoch.load(Ordering::Relaxed));

            drop(writer);
            assert_eq!(completed_epoch + 1, end_epoch.load(Ordering::Relaxed));
            phaser.reader_lock().flip();
            assert_eq!(if odd_phase { 0 } else { i64::MIN }, phaser.start_epoch.load(Ordering::Relaxed));
        }
    }
}
