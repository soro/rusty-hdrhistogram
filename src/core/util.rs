use crate::core::constants::F64_SIGN_MASK;
use crate::core::ReadableHistogram;

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

#[inline(always)]
pub fn normalize_index(index: u32, normalizing_index_offset: i32, array_length: u32) -> u32 {
    if normalizing_index_offset == 0 {
        return index;
    }
    let length = array_length as i64;
    let mut normalized = index as i64 - normalizing_index_offset as i64;
    if normalized < 0 {
        normalized += length;
    } else if normalized >= length {
        normalized -= length;
    }
    normalized as u32
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
