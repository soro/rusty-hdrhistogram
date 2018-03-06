use core::constants::F64_SIGN_MASK;
use core::ReadableHistogram;

// this will work for two's complement floats if ints have the same endianness on the platform as floats
pub fn next_below(value: f64) -> f64 {
    if value.is_nan() {
        return value;
    } else {
        let mut transient = value.to_bits();

        if value > 0.0 {
            transient -= 1;
        } else if value < 0.0 {
            transient += 1;
        } else {
            transient = F64_SIGN_MASK | 1;
        }

        f64::from_bits(transient)
    }
}

pub mod mem_util {
    use std::heap::{Alloc, Layout};
    use std::mem;

    #[inline(always)]
    pub fn get_layout<T>(length: usize) -> Layout {
        let t_size = mem::size_of::<T>();
        let size = length.checked_mul(t_size).expect("capacityoverflow");
        unsafe { Layout::from_size_align_unchecked(size, mem::align_of::<T>()) }
    }

    #[inline(always)]
    pub fn alloc_guard(alloc_size: usize) {
        if mem::size_of::<usize>() < 8 {
            assert!(
                alloc_size <= isize::max_value() as usize,
                "capacity overflow"
            );
        }
    }

    #[inline(always)]
    pub unsafe fn alloc_zeroed_array_in<T, A: Alloc>(length: u32, allocator: &mut A) -> *mut u8 {
        let t_size = mem::size_of::<T>();
        let size = (length as usize)
            .checked_mul(t_size)
            .expect("capacity overflow");

        alloc_guard(size);

        if size == 0 {
            mem::align_of::<T>() as *mut u8
        } else {
            match allocator.alloc_zeroed(get_layout::<T>(size)) {
                Ok(ptr) => ptr,
                Err(err) => allocator.oom(err),
            }
        }
    }
}

pub mod hashing {
    pub fn hash_mix(h: &mut i64) {
        *h += *h << 10;
        *h ^= *h >> 6;
    }

    pub fn add_mix32(h: &mut i64, t: u32) {
        *h += t as i64;
        hash_mix(h);
    }

    pub fn add_mix64(h: &mut i64, v: u64) {
        *h += v as i64;
        hash_mix(h);
    }
}

#[macro_export]
macro_rules! check_eq {
    ($left:expr, $right:expr) => {
        if $left != $right { return false; }
    }
}

pub fn recalculate_internal_tracking_values<H: ReadableHistogram>(histogram: &mut H, length_to_cover: u32) -> (Option<u32>, Option<u32>, u64) {
    let mut new_max = None;
    let mut new_min = None;
    let mut new_total = 0;
    for i in 0..length_to_cover {
        let count = histogram.unsafe_get_count_at_index(i);
        if count > 0 {
            new_total += count;
            new_max = Some(i);
            if new_min.is_none() && i != 0 {
                new_min = Some(i)
            }
        }
    }
    (new_max, new_min, new_total)
}

