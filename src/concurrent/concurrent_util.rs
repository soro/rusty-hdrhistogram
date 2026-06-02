use crate::core::*;
use std::sync::atomic::{AtomicU64, Ordering};

#[inline(always)]
pub fn update_max_value(layout: &HistogramLayout, raw_max_value: &AtomicU64, value: u64) {
    let internal_value = value | layout.unit_magnitude_mask;
    let mut sampled_max_value = raw_max_value.load(Ordering::Relaxed);
    while sampled_max_value < internal_value {
        if raw_max_value
            .compare_exchange_weak(sampled_max_value, internal_value, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            break;
        }
        sampled_max_value = raw_max_value.load(Ordering::Relaxed);
    }
}

#[inline(always)]
pub fn update_min_non_zero_value(layout: &HistogramLayout, raw_min_non_zero_value: &AtomicU64, value: u64) {
    if value <= layout.unit_magnitude_mask {
        return;
    }
    let internal_value = value & !layout.unit_magnitude_mask;
    let mut sampled_min_value = raw_min_non_zero_value.load(Ordering::Relaxed);
    while internal_value < sampled_min_value {
        if raw_min_non_zero_value
            .compare_exchange_weak(sampled_min_value, internal_value, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            break;
        }
        sampled_min_value = raw_min_non_zero_value.load(Ordering::Relaxed);
    }
}
