use crate::core::*;
use crate::core::constants::*;

#[derive(Clone, Debug, PartialEq)]
#[repr(C)]
pub struct HistogramSettings {
    pub auto_resize: bool,
    pub bucket_count: u32,
    pub counts_array_length: u32,
    pub double_to_integer_value_conversion_ratio: f64,
    pub integer_to_double_value_conversion_ratio: f64,
    pub highest_trackable_value: u64,
    pub lowest_discernible_value: u64,
    pub number_of_significant_value_digits: u32,
    pub sub_bucket_count: u32,
    pub leading_zero_count_base: u32,
    pub sub_bucket_mask: u64,
    pub unit_magnitude: u32,
    pub sub_bucket_half_count_magnitude: u32,
    pub sub_bucket_half_count: u32,
    pub unit_magnitude_mask: u64,
}

macro_rules! expect {
    ($t:expr, $e:expr) => {
        if $t { return Err($e); }
    };
}

#[allow(dead_code)]
impl HistogramSettings {
    pub fn new(
        lowest_discernible_value: u64,
        highest_trackable_value: u64,
        significant_value_digits: u8,
    ) -> Result<HistogramSettings, CreationError> {
        expect!(lowest_discernible_value < 1, CreationError::LowIsZero);
        expect!(
            lowest_discernible_value > u64::MAX / 2,
            CreationError::LowGtMax
        );
        expect!(
            highest_trackable_value < 2 * lowest_discernible_value,
            CreationError::HighLt2Low
        );
        expect!(
            significant_value_digits > 5,
            CreationError::SignificantValueDigitsExceedsMax
        );

        let largest_value_with_single_unit_resolution = 2 * 10_u64.pow(u32::from(significant_value_digits));

        let unit_magnitude = (lowest_discernible_value as f64).log2().floor() as u32;
        let unit_magnitude_mask = (1 << unit_magnitude) - 1;

        let sub_bucket_count_magnitude = (largest_value_with_single_unit_resolution as f64)
            .log2()
            .ceil() as u32;
        let sub_bucket_half_count_magnitude = sub_bucket_count_magnitude - 1;
        let sub_bucket_count = 1 << sub_bucket_count_magnitude;

        expect!(
            unit_magnitude + sub_bucket_count_magnitude > 63,
            CreationError::CantReprSigDigitsLtLowestDiscernible
        );

        let sub_bucket_half_count = sub_bucket_count / 2;

        let sub_bucket_mask = (sub_bucket_count as u64 - 1) << unit_magnitude;

        let mut s = HistogramSettings {
            auto_resize: false,
            bucket_count: 0,
            counts_array_length: 0,
            double_to_integer_value_conversion_ratio: 1.0,
            integer_to_double_value_conversion_ratio: 1.0,
            highest_trackable_value: highest_trackable_value,
            lowest_discernible_value: lowest_discernible_value,
            leading_zero_count_base: 64 - unit_magnitude - sub_bucket_count_magnitude,
            number_of_significant_value_digits: significant_value_digits as u32,
            sub_bucket_count,
            sub_bucket_half_count,
            sub_bucket_half_count_magnitude,
            sub_bucket_mask,
            unit_magnitude,
            unit_magnitude_mask,
        };

        let buckets_needed = s.get_buckets_needed_to_cover_value(highest_trackable_value);

        expect!(
            buckets_needed > i32::MAX as u32,
            CreationError::RequiresExcessiveArrayLen
        );
        s.bucket_count = buckets_needed;
        s.counts_array_length = s.get_length_for_number_of_buckets(s.bucket_count);

        Ok(s)
    }

    #[inline(always)]
    pub fn size_of_equivalent_value_range(&self, value: u64) -> u64 {
        let bucket_index = self.get_bucket_index(value);
        1_u64 << (self.unit_magnitude + bucket_index)
    }

    #[inline(always)]
    pub fn lowest_equivalent_value(&self, value: u64) -> u64 {
        let bucket_index = self.get_bucket_index(value);
        let sub_bucket_index = self.get_sub_bucket_index(value, bucket_index);
        self.value_from_indexes(bucket_index, sub_bucket_index)
    }

    #[inline(always)]
    pub fn highest_equivalent_value(&self, value: u64) -> u64 {
        if value == u64::MAX {
            u64::MAX
        } else {
            self.next_non_equivalent_value(value) - 1
        }
    }

    #[inline(always)]
    pub fn median_equivalent_value(&self, value: u64) -> u64 {
        self.lowest_equivalent_value(value) + (self.size_of_equivalent_value_range(value) >> 1)
    }

    #[inline(always)]
    pub fn next_non_equivalent_value(&self, value: u64) -> u64 {
        self.lowest_equivalent_value(value) + self.size_of_equivalent_value_range(value)
    }

    #[inline(always)]
    pub fn values_are_equivalent(&self, value1: u64, value2: u64) -> bool {
        self.lowest_equivalent_value(value1) == self.lowest_equivalent_value(value2)
    }

    #[inline(always)]
    pub fn get_bucket_index(&self, value: u64) -> u32 {
        self.leading_zero_count_base - (value | self.sub_bucket_mask).leading_zeros() as u32
    }

    #[inline(always)]
    pub fn get_sub_bucket_index(&self, value: u64, bucket_index: u32) -> u32 {
        (value >> (bucket_index + self.unit_magnitude)) as u32
    }

    #[inline(always)]
    pub fn value_from_index(&self, index: u32) -> u64 {
        let bucket_idx_succ = index >> self.sub_bucket_half_count_magnitude;
        let mut sub_bucket_index = (index & (self.sub_bucket_half_count - 1)) + self.sub_bucket_half_count;
        let bucket_index = if bucket_idx_succ == 0 {
            sub_bucket_index -= self.sub_bucket_half_count;
            0
        } else {
            bucket_idx_succ - 1
        };
        self.value_from_indexes(bucket_index, sub_bucket_index)
    }

    #[inline(always)]
    pub fn value_from_indexes(&self, bucket_index: u32, sub_bucket_index: u32) -> u64 {
        (sub_bucket_index as u64) << (bucket_index + self.unit_magnitude)
    }

    #[inline(always)]
    pub fn get_buckets_needed_to_cover_value(&self, value: u64) -> u32 {
        let mut smallest_untrackable_value = (self.sub_bucket_count as u64) << self.unit_magnitude;

        let mut buckets_needed = 1;
        while smallest_untrackable_value <= value {
            if smallest_untrackable_value > u64::MAX / 2 {
                return buckets_needed + 1;
            }
            smallest_untrackable_value <<= 1;
            buckets_needed += 1;
        }
        buckets_needed
    }

    #[inline(always)]
    pub fn get_length_for_number_of_buckets(&self, number_of_buckets: u32) -> u32 {
        (number_of_buckets + 1) * (self.sub_bucket_half_count)
    }

    #[inline(always)]
    pub fn determine_array_length_needed(&self, value: u64) -> u32 {
        let buckets_required = self.get_buckets_needed_to_cover_value(value);
        self.get_length_for_number_of_buckets(buckets_required)
    }

    #[inline(always)]
    pub fn counts_array_index(&self, value: u64) -> u32 {
        let bucket_index = self.get_bucket_index(value);
        let sub_bucket_index = self.get_sub_bucket_index(value, bucket_index);
        let bucket_base_index = ((bucket_index + 1) << self.sub_bucket_half_count_magnitude) as isize;
        let offset_in_bucket = sub_bucket_index as isize - self.sub_bucket_half_count as isize;

        (bucket_base_index + offset_in_bucket) as u32
    }

    #[inline(always)]
    pub fn is_add_compatible_with(&self, other: &HistogramSettings) -> bool {
        self.bucket_count == other.bucket_count && self.sub_bucket_count == other.sub_bucket_count
            && self.unit_magnitude == other.unit_magnitude
    }

    // this function is only called in settings where new_highest_trackable is > self.highest_trackable, but not safe in general
    #[inline(never)]
    pub fn resize(&mut self, value: u64) -> Result<u32, CreationError> {
        let new_bucket_count = self.get_buckets_needed_to_cover_value(value);
        let new_length = self.get_length_for_number_of_buckets(new_bucket_count);

        if new_length > i32::MAX as u32 {
            Err(CreationError::RequiresExcessiveArrayLen)
        } else {
            self.bucket_count = new_bucket_count;

            let new_highest = self.highest_equivalent_value(self.value_from_index(new_length - 1));

            self.highest_trackable_value = new_highest;
            self.counts_array_length = new_length;

            Ok(new_length)
        }
    }

    pub fn get_max_value(&self, raw_max: u64) -> u64 {
        if raw_max == ORIGINAL_MAX {
            ORIGINAL_MAX
        } else {
            self.highest_equivalent_value(raw_max)
        }
    }

    pub fn get_min_non_zero_value(&self, raw_min_non_zero: u64) -> u64 {
        if raw_min_non_zero == ORIGINAL_MIN {
            ORIGINAL_MIN
        } else {
            self.lowest_equivalent_value(raw_min_non_zero)
        }
    }

    pub fn equals(&self, other: &Self) -> bool {
        !(self.lowest_discernible_value != other.lowest_discernible_value
            || self.number_of_significant_value_digits != other.number_of_significant_value_digits
            || self.integer_to_double_value_conversion_ratio != other.integer_to_double_value_conversion_ratio)
    }
}
