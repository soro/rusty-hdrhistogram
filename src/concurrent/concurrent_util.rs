use std::sync::atomic::{Ordering, AtomicU64};
use std::cell::UnsafeCell;
use crate::core::*;

#[inline(always)]
pub fn update_max_value(settings: &UnsafeCell<HistogramSettings>, raw_max_value: &AtomicU64, value: u64) {
    let internal_value = value | unsafe { (*settings.get()).unit_magnitude_mask };
    let mut sampled_max_value = raw_max_value.load(Ordering::Relaxed);
    while sampled_max_value < internal_value {
        match raw_max_value.compare_exchange_weak(
            sampled_max_value,
            internal_value,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(_) => (),
        }
        sampled_max_value = raw_max_value.load(Ordering::Relaxed);
    }
}

#[inline(always)]
pub fn update_min_non_zero_value(settings: &UnsafeCell<HistogramSettings>, raw_min_non_zero_value: &AtomicU64, value: u64) {
    unsafe {
        let settings = settings.get();
        if value <= (*settings).unit_magnitude_mask {
            return;
        }
        let internal_value = value & !(*settings).unit_magnitude_mask;
        let mut sampled_min_value = raw_min_non_zero_value.load(Ordering::Relaxed);
    while internal_value < sampled_min_value {
        match raw_min_non_zero_value.compare_exchange_weak(
            sampled_min_value,
            internal_value,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(_) => (),
            }
            sampled_min_value = raw_min_non_zero_value.load(Ordering::Relaxed);
        }
    }
}
