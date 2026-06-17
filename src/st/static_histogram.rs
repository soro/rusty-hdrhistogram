use crate::core::constants::*;
use crate::core::counter::Counter;
use crate::core::meta_data::HistogramMetaData;
use crate::core::*;
use crate::iteration::*;
use std::borrow::Borrow;

const fn pow10_u64(exp: u8) -> u64 {
    let mut result = 1_u64;
    let mut i = 0_u8;
    while i < exp {
        result *= 10;
        i += 1;
    }
    result
}

const fn validate_static_layout(lowest_discernible_value: u64, highest_trackable_value: u64, significant_value_digits: u8) {
    if lowest_discernible_value < 1 {
        panic!("lowest discernible value must be greater than zero");
    }
    if lowest_discernible_value > u64::MAX / 2 {
        panic!("lowest discernible value is too large");
    }
    if highest_trackable_value < 2 * lowest_discernible_value {
        panic!("highest trackable value must be at least twice the lowest discernible value");
    }
    if significant_value_digits > 5 {
        panic!("significant value digits must be at most 5");
    }
}

const fn static_unit_magnitude(lowest_discernible_value: u64, highest_trackable_value: u64, significant_value_digits: u8) -> u32 {
    validate_static_layout(lowest_discernible_value, highest_trackable_value, significant_value_digits);
    floor_log2_u64(lowest_discernible_value)
}

const fn static_sub_bucket_count_magnitude(
    lowest_discernible_value: u64,
    highest_trackable_value: u64,
    significant_value_digits: u8,
) -> u32 {
    validate_static_layout(lowest_discernible_value, highest_trackable_value, significant_value_digits);
    let largest_value_with_single_unit_resolution = 2 * pow10_u64(significant_value_digits);
    let sub_bucket_count_magnitude = ceil_log2_u64(largest_value_with_single_unit_resolution);
    let unit_magnitude = floor_log2_u64(lowest_discernible_value);
    if unit_magnitude + sub_bucket_count_magnitude > 63 {
        panic!("cannot represent the requested significant digits at the lowest discernible value");
    }
    sub_bucket_count_magnitude
}

const fn static_sub_bucket_half_count(lowest_discernible_value: u64, highest_trackable_value: u64, significant_value_digits: u8) -> u32 {
    1_u32 << (static_sub_bucket_count_magnitude(lowest_discernible_value, highest_trackable_value, significant_value_digits) - 1)
}

const fn static_buckets_needed_to_cover_value(
    lowest_discernible_value: u64,
    highest_trackable_value: u64,
    significant_value_digits: u8,
) -> u32 {
    let unit_magnitude = static_unit_magnitude(lowest_discernible_value, highest_trackable_value, significant_value_digits);
    let sub_bucket_count_magnitude =
        static_sub_bucket_count_magnitude(lowest_discernible_value, highest_trackable_value, significant_value_digits);
    let sub_bucket_count = 1_u32 << sub_bucket_count_magnitude;
    let mut smallest_untrackable_value = (sub_bucket_count as u64) << unit_magnitude;
    let mut buckets_needed = 1_u32;

    while smallest_untrackable_value <= highest_trackable_value {
        if smallest_untrackable_value > u64::MAX / 2 {
            return buckets_needed + 1;
        }
        smallest_untrackable_value <<= 1;
        buckets_needed += 1;
    }

    buckets_needed
}

/// Return the HdrHistogram counts-array length required for a static histogram
/// with the given layout parameters.
///
/// This function is `const`, so it can be used in const generic arguments and
/// by [`static_histogram!`](crate::static_histogram).
pub const fn static_histogram_counts_array_length(
    lowest_discernible_value: u64,
    highest_trackable_value: u64,
    significant_value_digits: u8,
) -> usize {
    let bucket_count = static_buckets_needed_to_cover_value(lowest_discernible_value, highest_trackable_value, significant_value_digits);
    if bucket_count > i32::MAX as u32 {
        panic!("requested range requires an excessive counts array length");
    }
    let sub_bucket_half_count = static_sub_bucket_half_count(lowest_discernible_value, highest_trackable_value, significant_value_digits);
    ((bucket_count + 1) * sub_bucket_half_count) as usize
}

#[repr(C)]
pub struct StaticHistogramWithCounter<
    T,
    const LOWEST_DISCERNIBLE_VALUE: u64,
    const HIGHEST_TRACKABLE_VALUE: u64,
    const SIGNIFICANT_VALUE_DIGITS: u8,
    const N: usize,
> {
    pub meta_data: HistogramMetaData,
    raw_max_value: u64,
    raw_min_non_zero_value: u64,
    total_count: u64,
    counts: [T; N],
}

/// Fixed-range integer histogram with `u64` counters and all layout constants
/// encoded in its type.
///
/// Use [`static_histogram!`](crate::static_histogram) to define ergonomic named
/// aliases without manually providing the count-array length const parameter.
pub type StaticHistogram<
    const LOWEST_DISCERNIBLE_VALUE: u64,
    const HIGHEST_TRACKABLE_VALUE: u64,
    const SIGNIFICANT_VALUE_DIGITS: u8,
    const N: usize,
> = StaticHistogramWithCounter<u64, LOWEST_DISCERNIBLE_VALUE, HIGHEST_TRACKABLE_VALUE, SIGNIFICANT_VALUE_DIGITS, N>;

impl<
        T: Counter,
        const LOWEST_DISCERNIBLE_VALUE: u64,
        const HIGHEST_TRACKABLE_VALUE: u64,
        const SIGNIFICANT_VALUE_DIGITS: u8,
        const N: usize,
    > StaticHistogramWithCounter<T, LOWEST_DISCERNIBLE_VALUE, HIGHEST_TRACKABLE_VALUE, SIGNIFICANT_VALUE_DIGITS, N>
{
    const UNIT_MAGNITUDE: u32 = static_unit_magnitude(LOWEST_DISCERNIBLE_VALUE, HIGHEST_TRACKABLE_VALUE, SIGNIFICANT_VALUE_DIGITS);
    const SUB_BUCKET_COUNT_MAGNITUDE: u32 =
        static_sub_bucket_count_magnitude(LOWEST_DISCERNIBLE_VALUE, HIGHEST_TRACKABLE_VALUE, SIGNIFICANT_VALUE_DIGITS);
    const SUB_BUCKET_HALF_COUNT_MAGNITUDE: u32 = Self::SUB_BUCKET_COUNT_MAGNITUDE - 1;
    const SUB_BUCKET_COUNT: u32 = 1_u32 << Self::SUB_BUCKET_COUNT_MAGNITUDE;
    const SUB_BUCKET_HALF_COUNT: u32 = 1_u32 << Self::SUB_BUCKET_HALF_COUNT_MAGNITUDE;
    const LEADING_ZERO_COUNT_BASE: u32 = 64 - Self::UNIT_MAGNITUDE - Self::SUB_BUCKET_COUNT_MAGNITUDE;
    const SUB_BUCKET_MASK: u64 = (Self::SUB_BUCKET_COUNT as u64 - 1) << Self::UNIT_MAGNITUDE;
    const UNIT_MAGNITUDE_MASK: u64 = (1_u64 << Self::UNIT_MAGNITUDE) - 1;
    const BUCKET_COUNT: u32 =
        static_buckets_needed_to_cover_value(LOWEST_DISCERNIBLE_VALUE, HIGHEST_TRACKABLE_VALUE, SIGNIFICANT_VALUE_DIGITS);
    const COUNTS_ARRAY_LENGTH: u32 =
        static_histogram_counts_array_length(LOWEST_DISCERNIBLE_VALUE, HIGHEST_TRACKABLE_VALUE, SIGNIFICANT_VALUE_DIGITS) as u32;
    const CAPACITY_CHECK: () = assert!(N >= Self::COUNTS_ARRAY_LENGTH as usize);

    pub fn new() -> Self {
        let _ = Self::CAPACITY_CHECK;
        StaticHistogramWithCounter {
            meta_data: HistogramMetaData::new(),
            raw_max_value: ORIGINAL_MAX,
            raw_min_non_zero_value: ORIGINAL_MIN,
            total_count: 0,
            counts: [T::zero(); N],
        }
    }

    /// Return a snapshot of the settings that define this histogram's precision
    /// and fixed trackable range.
    pub fn settings(&self) -> HistogramSettings {
        HistogramSettings {
            auto_resize: false,
            bucket_count: Self::BUCKET_COUNT,
            counts_array_length: Self::COUNTS_ARRAY_LENGTH,
            highest_trackable_value: HIGHEST_TRACKABLE_VALUE,
            lowest_discernible_value: LOWEST_DISCERNIBLE_VALUE,
            number_of_significant_value_digits: SIGNIFICANT_VALUE_DIGITS as u32,
            sub_bucket_count: Self::SUB_BUCKET_COUNT,
            leading_zero_count_base: Self::LEADING_ZERO_COUNT_BASE,
            sub_bucket_mask: Self::SUB_BUCKET_MASK,
            unit_magnitude: Self::UNIT_MAGNITUDE,
            sub_bucket_half_count_magnitude: Self::SUB_BUCKET_HALF_COUNT_MAGNITUDE,
            sub_bucket_half_count: Self::SUB_BUCKET_HALF_COUNT,
            unit_magnitude_mask: Self::UNIT_MAGNITUDE_MASK,
        }
    }

    pub fn get_count_at_index(&self, index: u32) -> Option<&T> {
        if index >= Self::COUNTS_ARRAY_LENGTH {
            return None;
        }
        Some(self.unsafe_get_count_at_index(index))
    }

    #[inline(always)]
    pub(crate) fn unsafe_get_count_at_index(&self, index: u32) -> &T {
        unsafe { self.counts.get_unchecked(index as usize) }
    }

    #[inline(always)]
    fn unsafe_get_count_at_index_mut(&mut self, index: u32) -> &mut T {
        unsafe { self.counts.get_unchecked_mut(index as usize) }
    }

    pub fn get_total_count(&self) -> u64 {
        self.total_count
    }

    #[inline(always)]
    pub fn counts_array_length(&self) -> u32 {
        Self::COUNTS_ARRAY_LENGTH
    }

    #[inline(always)]
    pub(crate) fn normalizing_index_offset(&self) -> i32 {
        0
    }

    #[inline(always)]
    pub fn counts_array_index(&self, value: u64) -> u32 {
        Self::counts_array_index_for(value)
    }

    pub fn get_count_at_value(&self, value: u64) -> Option<T> {
        let idx = Self::counts_array_index_for(value);
        if idx < Self::COUNTS_ARRAY_LENGTH {
            Some(*self.unsafe_get_count_at_index(idx))
        } else {
            None
        }
    }

    pub fn supports_auto_resize(&self) -> bool {
        false
    }

    pub fn hash_code(&self) -> i64 {
        use crate::core::util::hashing::*;
        let mut h = 0_i64;
        add_mix32(&mut h, Self::UNIT_MAGNITUDE);
        add_mix32(&mut h, SIGNIFICANT_VALUE_DIGITS as u32);
        add_mix64(&mut h, self.total_count);
        add_mix64(&mut h, self.raw_max_value);
        add_mix64(&mut h, self.raw_min_non_zero_value);
        h += h << 3;
        h ^= h >> 11;
        h += h << 15;
        h
    }

    pub fn equals(&self, other: &Self) -> bool {
        if std::ptr::eq(self, other) {
            return true;
        }
        if self.get_total_count() != other.get_total_count() {
            return false;
        }
        if self.get_max_value() != other.get_max_value() {
            return false;
        }
        if self.get_min_non_zero_value() != other.get_min_non_zero_value() {
            return false;
        }
        for i in 0..Self::COUNTS_ARRAY_LENGTH {
            if self.unsafe_get_count_at_index(i) != other.unsafe_get_count_at_index(i) {
                return false;
            }
        }
        true
    }

    #[inline(always)]
    pub fn is_auto_resize(&self) -> bool {
        false
    }

    pub fn get_min_value(&self) -> u64 {
        if self.get_total_count() == 0 || *self.unsafe_get_count_at_index(0) != T::zero() {
            0
        } else {
            Self::lowest_equivalent_value_for(self.raw_min_non_zero_value)
        }
    }

    pub fn get_max_value(&self) -> u64 {
        Self::max_value_for_raw(self.raw_max_value)
    }

    pub fn get_min_non_zero_value(&self) -> u64 {
        Self::min_non_zero_value_for_raw(self.raw_min_non_zero_value)
    }

    pub fn get_mean(&self) -> f64 {
        RecordedValuesIterator::get_mean_without_reset(&mut self.recorded_values())
    }

    pub fn get_std_deviation(&self) -> f64 {
        RecordedValuesIterator::get_std_deviation_without_reset(&mut self.recorded_values())
    }

    pub fn get_value_at_percentile(&self, percentile: f64) -> u64 {
        let one_below = util::next_below(percentile);
        let requested_percentile = one_below.clamp(0.0, 100.0);

        let fractional_count = (requested_percentile / 100.0) * self.total_count as f64;
        let mut count_at_percentile = fractional_count.ceil() as u64;
        count_at_percentile = std::cmp::max(count_at_percentile, 1);

        let mut total_to_current_index: u64 = 0;
        for i in 0..Self::COUNTS_ARRAY_LENGTH {
            total_to_current_index += self.unsafe_get_count_at_index(i).as_u64();
            if total_to_current_index >= count_at_percentile {
                let value_at_index = Self::value_from_index_for(i);
                return if percentile == 0.0 {
                    Self::lowest_equivalent_value_for(value_at_index)
                } else {
                    Self::highest_equivalent_value_for(value_at_index)
                };
            }
        }

        0
    }

    pub fn get_percentile_at_or_below_value(&self, value: u64) -> f64 {
        if self.total_count == 0 {
            return 100.0;
        }

        let target_index = self.saturating_counts_array_index(value);
        let total_to_current_index = (0..=target_index).fold(0_f64, |t, i| t + self.unsafe_get_count_at_index(i).as_f64());
        (100.0 * total_to_current_index) / self.total_count as f64
    }

    pub fn get_lowest_discernible_value(&self) -> u64 {
        LOWEST_DISCERNIBLE_VALUE
    }

    #[deprecated(note = "use get_lowest_discernible_value")]
    pub fn get_lowest_discernable_value(&self) -> u64 {
        self.get_lowest_discernible_value()
    }

    pub fn get_highest_trackable_value(&self) -> u64 {
        HIGHEST_TRACKABLE_VALUE
    }

    pub fn get_number_of_significant_value_digits(&self) -> u32 {
        SIGNIFICANT_VALUE_DIGITS as u32
    }

    #[inline(always)]
    pub(crate) fn integer_to_double_value_conversion_ratio(&self) -> f64 {
        1.0
    }

    #[inline(always)]
    pub(crate) fn double_to_integer_value_conversion_ratio(&self) -> f64 {
        1.0
    }

    #[inline(always)]
    pub(crate) fn lowest_tracking_integer_value(&self) -> u64 {
        Self::SUB_BUCKET_HALF_COUNT as u64
    }

    pub fn lowest_equivalent_value(&self, value: u64) -> u64 {
        Self::lowest_equivalent_value_for(value)
    }

    pub fn highest_equivalent_value(&self, value: u64) -> u64 {
        Self::highest_equivalent_value_for(value)
    }

    pub fn median_equivalent_value(&self, value: u64) -> u64 {
        Self::median_equivalent_value_for(value)
    }

    pub fn next_non_equivalent_value(&self, value: u64) -> u64 {
        Self::next_non_equivalent_value_for(value)
    }

    pub fn size_of_equivalent_value_range(&self, value: u64) -> u64 {
        Self::size_of_equivalent_value_range_for(value)
    }

    pub fn values_are_equivalent(&self, v1: u64, v2: u64) -> bool {
        Self::lowest_equivalent_value_for(v1) == Self::lowest_equivalent_value_for(v2)
    }

    pub fn value_from_index(&self, index: u32) -> u64 {
        Self::value_from_index_for(index)
    }

    #[inline(always)]
    fn get_bucket_index(value: u64) -> u32 {
        Self::LEADING_ZERO_COUNT_BASE - (value | Self::SUB_BUCKET_MASK).leading_zeros()
    }

    #[inline(always)]
    fn get_sub_bucket_index(value: u64, bucket_index: u32) -> u32 {
        (value >> (bucket_index + Self::UNIT_MAGNITUDE)) as u32
    }

    #[inline(always)]
    fn value_from_indexes(bucket_index: u32, sub_bucket_index: u32) -> u64 {
        (sub_bucket_index as u64) << (bucket_index + Self::UNIT_MAGNITUDE)
    }

    #[inline(always)]
    fn value_from_index_for(index: u32) -> u64 {
        let bucket_idx_succ = index >> Self::SUB_BUCKET_HALF_COUNT_MAGNITUDE;
        let mut sub_bucket_index = (index & (Self::SUB_BUCKET_HALF_COUNT - 1)) + Self::SUB_BUCKET_HALF_COUNT;
        let bucket_index = if bucket_idx_succ == 0 {
            sub_bucket_index -= Self::SUB_BUCKET_HALF_COUNT;
            0
        } else {
            bucket_idx_succ - 1
        };
        Self::value_from_indexes(bucket_index, sub_bucket_index)
    }

    #[inline(always)]
    fn counts_array_index_for(value: u64) -> u32 {
        let bucket_index = Self::get_bucket_index(value);
        let sub_bucket_index = Self::get_sub_bucket_index(value, bucket_index);
        let bucket_base_index = ((bucket_index + 1) << Self::SUB_BUCKET_HALF_COUNT_MAGNITUDE) as isize;
        let offset_in_bucket = sub_bucket_index as isize - Self::SUB_BUCKET_HALF_COUNT as isize;

        (bucket_base_index + offset_in_bucket) as u32
    }

    #[inline(always)]
    fn size_of_equivalent_value_range_for(value: u64) -> u64 {
        let bucket_index = Self::get_bucket_index(value);
        1_u64 << (Self::UNIT_MAGNITUDE + bucket_index)
    }

    #[inline(always)]
    fn lowest_equivalent_value_for(value: u64) -> u64 {
        let bucket_index = Self::get_bucket_index(value);
        let sub_bucket_index = Self::get_sub_bucket_index(value, bucket_index);
        Self::value_from_indexes(bucket_index, sub_bucket_index)
    }

    #[inline(always)]
    fn highest_equivalent_value_for(value: u64) -> u64 {
        if value == u64::MAX {
            u64::MAX
        } else {
            Self::next_non_equivalent_value_for(value) - 1
        }
    }

    #[inline(always)]
    fn median_equivalent_value_for(value: u64) -> u64 {
        Self::lowest_equivalent_value_for(value) + (Self::size_of_equivalent_value_range_for(value) >> 1)
    }

    #[inline(always)]
    fn next_non_equivalent_value_for(value: u64) -> u64 {
        Self::lowest_equivalent_value_for(value) + Self::size_of_equivalent_value_range_for(value)
    }

    #[inline(always)]
    fn max_value_for_raw(raw_max: u64) -> u64 {
        if raw_max == ORIGINAL_MAX {
            ORIGINAL_MAX
        } else {
            Self::highest_equivalent_value_for(raw_max)
        }
    }

    #[inline(always)]
    fn min_non_zero_value_for_raw(raw_min_non_zero: u64) -> u64 {
        if raw_min_non_zero == ORIGINAL_MIN {
            ORIGINAL_MIN
        } else {
            Self::lowest_equivalent_value_for(raw_min_non_zero)
        }
    }

    #[inline(always)]
    fn add_to_count_at_index(&mut self, idx: u32, count: T) {
        *self.unsafe_get_count_at_index_mut(idx) += count;
    }

    #[inline(always)]
    fn set_count_at_index(&mut self, idx: u32, count: T) {
        *self.unsafe_get_count_at_index_mut(idx) = count;
    }

    fn update_max_value(&mut self, value: u64) {
        let internal_value = value | Self::UNIT_MAGNITUDE_MASK;
        if internal_value > self.raw_max_value {
            self.raw_max_value = internal_value;
        }
    }

    fn reset_max_value(&mut self, max_value: u64) {
        self.raw_max_value = max_value | Self::UNIT_MAGNITUDE_MASK;
    }

    fn update_min_non_zero_value(&mut self, value: u64) {
        if value <= Self::UNIT_MAGNITUDE_MASK {
            return;
        }

        let internal_value = value & !Self::UNIT_MAGNITUDE_MASK;
        if internal_value < self.raw_min_non_zero_value {
            self.raw_min_non_zero_value = internal_value;
        }
    }

    fn reset_min_non_zero_value(&mut self, min_non_zero_value: u64) {
        let internal_value = min_non_zero_value & !Self::UNIT_MAGNITUDE_MASK;
        self.raw_min_non_zero_value = if min_non_zero_value == u64::MAX {
            min_non_zero_value
        } else {
            internal_value
        };
    }

    fn update_min_and_max(&mut self, value: u64) {
        if value > self.raw_max_value {
            self.update_max_value(value)
        }
        if value < self.raw_min_non_zero_value {
            self.update_min_non_zero_value(value)
        }
    }

    #[inline(always)]
    pub fn record_value(&mut self, value: u64) -> Result<(), RecordError> {
        self.record_single_value(value)
    }

    #[inline(always)]
    pub fn record_value_with_count(&mut self, value: u64, count: T) -> Result<(), RecordError> {
        self.record_count_at_value(count, value)
    }

    #[inline(always)]
    fn record_single_value(&mut self, value: u64) -> Result<(), RecordError> {
        let idx = Self::counts_array_index_for(value);

        if idx < Self::COUNTS_ARRAY_LENGTH {
            self.add_to_count_at_index(idx, T::one());
            self.update_min_and_max(value);
            self.total_count += 1;
            Ok(())
        } else {
            Err(RecordError::ValueOutOfRangeResizeDisabled)
        }
    }

    #[inline(always)]
    fn record_count_at_value(&mut self, count: T, value: u64) -> Result<(), RecordError> {
        let idx = Self::counts_array_index_for(value);

        if idx < Self::COUNTS_ARRAY_LENGTH {
            self.add_to_count_at_index(idx, count);
            self.update_min_and_max(value);
            self.total_count += count.as_u64();
            Ok(())
        } else {
            Err(RecordError::ValueOutOfRangeResizeDisabled)
        }
    }

    #[inline]
    pub fn record_value_with_count_and_expected_interval(
        &mut self,
        value: u64,
        count: T,
        expected_interval_between_value_samples: u64,
    ) -> Result<(), RecordError> {
        self.record_count_at_value(count, value)?;
        if expected_interval_between_value_samples != 0 && value > expected_interval_between_value_samples {
            let mut missing_value = value - expected_interval_between_value_samples;
            while missing_value >= expected_interval_between_value_samples {
                self.record_count_at_value(count, missing_value)?;
                missing_value -= expected_interval_between_value_samples;
            }
        }

        Ok(())
    }

    pub fn record_value_with_expected_interval(
        &mut self,
        value: u64,
        expected_interval_between_value_samples: u64,
    ) -> Result<(), RecordError> {
        self.record_value_with_count_and_expected_interval(value, T::one(), expected_interval_between_value_samples)
    }

    pub fn add<B: Borrow<Self>>(&mut self, other_histogram: B) -> Result<(), RecordError> {
        let other_histogram = other_histogram.borrow();
        let mut observed_other_total_count: u64 = 0;
        for i in 0..Self::COUNTS_ARRAY_LENGTH {
            let other_count = *other_histogram.unsafe_get_count_at_index(i);
            if other_count != T::zero() {
                self.add_to_count_at_index(i, other_count);
                observed_other_total_count += other_count.as_u64();
            }
        }

        self.total_count += observed_other_total_count;
        self.update_max_value(other_histogram.get_max_value());
        self.update_min_non_zero_value(other_histogram.get_min_non_zero_value());
        Ok(())
    }

    pub fn subtract<B: Borrow<Self>>(&mut self, other_histogram: B) -> Result<(), SubtractionError> {
        let other_histogram = other_histogram.borrow();

        // This mirrors HistogramWithCounter's fused validation/subtraction
        // path: a later count error can leave earlier buckets subtracted.
        for i in 0..Self::COUNTS_ARRAY_LENGTH {
            let other_count = *other_histogram.unsafe_get_count_at_index(i);
            if other_count != T::zero() {
                if *self.unsafe_get_count_at_index(i) < other_count {
                    return Err(SubtractionError::CountExceededAtValue);
                }
                let count = self.unsafe_get_count_at_index_mut(i);
                *count -= other_count;
            }
        }

        self.establish_internal_tracking_values();
        Ok(())
    }

    fn establish_internal_tracking_values(&mut self) {
        self.reset_max_value(ORIGINAL_MAX);
        self.reset_min_non_zero_value(ORIGINAL_MIN);
        let (new_max, new_min, new_total) = util::recalculate_internal_tracking_values(self, Self::COUNTS_ARRAY_LENGTH);
        if let Some(mi) = new_max {
            let new_max = Self::highest_equivalent_value_for(Self::value_from_index_for(mi));
            self.update_max_value(new_max);
        }
        if let Some(mi) = new_min {
            let new_min = Self::value_from_index_for(mi);
            self.update_min_non_zero_value(new_min);
        }
        self.total_count = new_total;
    }

    pub fn reset(&mut self) {
        for value in &mut self.counts[..Self::COUNTS_ARRAY_LENGTH as usize] {
            *value = T::zero();
        }
        self.reset_max_value(ORIGINAL_MAX);
        self.reset_min_non_zero_value(ORIGINAL_MIN);
        self.total_count = 0;
        self.meta_data.clear();
    }

    pub fn percentiles(&self, percentile_ticks_per_half_distance: u32) -> PercentileIterator<&'_ Self> {
        PercentileIterator::new(self, percentile_ticks_per_half_distance)
    }

    pub fn linear_bucket_values(&self, value_units_per_bucket: u64) -> LinearIterator<&'_ Self> {
        LinearIterator::new(self, value_units_per_bucket)
    }

    pub fn logarithmic_bucket_values(&self, value_units_in_first_bucket: u64, log_base: f64) -> LogarithmicIterator<&'_ Self> {
        LogarithmicIterator::new(self, value_units_in_first_bucket, log_base)
    }

    pub fn all_values(&self) -> AllValuesIterator<&'_ Self> {
        AllValuesIterator::new(self)
    }

    pub fn recorded_values(&self) -> RecordedValuesIterator<&'_ Self> {
        RecordedValuesIterator::new(self)
    }

    pub fn get_counts_slice(&self, length: u32) -> Option<&[T]> {
        let length = length as usize;
        if length <= Self::COUNTS_ARRAY_LENGTH as usize {
            Some(&self.counts[..length])
        } else {
            None
        }
    }

    pub(crate) fn get_counts_slice_mut(&mut self, length: u32) -> Option<&mut [T]> {
        let length = length as usize;
        if length <= Self::COUNTS_ARRAY_LENGTH as usize {
            Some(&mut self.counts[..length])
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn saturating_counts_array_index(&self, value: u64) -> u32 {
        let idx = Self::counts_array_index_for(value);
        let max_idx = Self::COUNTS_ARRAY_LENGTH - 1;
        if idx > max_idx {
            max_idx
        } else {
            idx
        }
    }

    #[inline(always)]
    pub fn last_index(&self) -> u32 {
        Self::COUNTS_ARRAY_LENGTH - 1
    }
}

impl<
        T: Counter,
        const LOWEST_DISCERNIBLE_VALUE: u64,
        const HIGHEST_TRACKABLE_VALUE: u64,
        const SIGNIFICANT_VALUE_DIGITS: u8,
        const N: usize,
    > crate::core::readable_histogram::sealed::Sealed
    for StaticHistogramWithCounter<T, LOWEST_DISCERNIBLE_VALUE, HIGHEST_TRACKABLE_VALUE, SIGNIFICANT_VALUE_DIGITS, N>
{
}

impl<
        T: Counter,
        const LOWEST_DISCERNIBLE_VALUE: u64,
        const HIGHEST_TRACKABLE_VALUE: u64,
        const SIGNIFICANT_VALUE_DIGITS: u8,
        const N: usize,
    > ReadableHistogram for StaticHistogramWithCounter<T, LOWEST_DISCERNIBLE_VALUE, HIGHEST_TRACKABLE_VALUE, SIGNIFICANT_VALUE_DIGITS, N>
{
    fn settings(&self) -> HistogramSettings {
        self.settings()
    }

    #[inline(always)]
    fn array_length(&self) -> u32 {
        self.counts_array_length()
    }

    #[inline(always)]
    fn get_total_count(&self) -> u64 {
        StaticHistogramWithCounter::<T, LOWEST_DISCERNIBLE_VALUE, HIGHEST_TRACKABLE_VALUE, SIGNIFICANT_VALUE_DIGITS, N>::get_total_count(
            self,
        )
    }

    #[inline(always)]
    fn unsafe_get_count_at_index(&self, idx: u32) -> u64 {
        StaticHistogramWithCounter::<T, LOWEST_DISCERNIBLE_VALUE, HIGHEST_TRACKABLE_VALUE, SIGNIFICANT_VALUE_DIGITS, N>::
            unsafe_get_count_at_index(self, idx)
            .as_u64()
    }

    fn get_max_value(&self) -> u64 {
        StaticHistogramWithCounter::<T, LOWEST_DISCERNIBLE_VALUE, HIGHEST_TRACKABLE_VALUE, SIGNIFICANT_VALUE_DIGITS, N>::get_max_value(self)
    }

    fn meta_data(&self) -> &HistogramMetaData {
        &self.meta_data
    }
}

impl<
        T: Counter,
        const LOWEST_DISCERNIBLE_VALUE: u64,
        const HIGHEST_TRACKABLE_VALUE: u64,
        const SIGNIFICANT_VALUE_DIGITS: u8,
        const N: usize,
    > IterableHistogram for StaticHistogramWithCounter<T, LOWEST_DISCERNIBLE_VALUE, HIGHEST_TRACKABLE_VALUE, SIGNIFICANT_VALUE_DIGITS, N>
{
}

impl<
        T: Counter,
        const LOWEST_DISCERNIBLE_VALUE: u64,
        const HIGHEST_TRACKABLE_VALUE: u64,
        const SIGNIFICANT_VALUE_DIGITS: u8,
        const N: usize,
    > EncodableHistogram for StaticHistogramWithCounter<T, LOWEST_DISCERNIBLE_VALUE, HIGHEST_TRACKABLE_VALUE, SIGNIFICANT_VALUE_DIGITS, N>
{
}

impl<
        T: Counter,
        const LOWEST_DISCERNIBLE_VALUE: u64,
        const HIGHEST_TRACKABLE_VALUE: u64,
        const SIGNIFICANT_VALUE_DIGITS: u8,
        const N: usize,
    > PartialEq for StaticHistogramWithCounter<T, LOWEST_DISCERNIBLE_VALUE, HIGHEST_TRACKABLE_VALUE, SIGNIFICANT_VALUE_DIGITS, N>
{
    fn eq(&self, other: &Self) -> bool {
        self.equals(other)
    }
}

#[macro_export]
macro_rules! static_histogram {
    (
        $(#[$meta:meta])*
        $vis:vis type $name:ident = {
            lowest_discernible_value: $lowest_discernible_value:expr,
            highest_trackable_value: $highest_trackable_value:expr,
            significant_digits: $significant_digits:expr $(,)?
        };
    ) => {
        $(#[$meta])*
        $vis type $name = $crate::st::StaticHistogram<
            { $lowest_discernible_value },
            { $highest_trackable_value },
            { $significant_digits },
            {
                $crate::st::static_histogram_counts_array_length(
                    $lowest_discernible_value,
                    $highest_trackable_value,
                    $significant_digits,
                )
            },
        >;
    };
}
