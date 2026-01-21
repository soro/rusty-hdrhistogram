use crate::core::*;
use crate::st::backing_array::BackingArray;
use crate::core::constants::*;
use crate::core::counter::Counter;
use crate::core::meta_data::HistogramMetaData;
use crate::iteration::*;
use std;
use std::borrow::Borrow;

#[repr(C)]
pub struct Histogram<T> {
    pub meta_data: HistogramMetaData,
    settings: HistogramSettings,
    raw_max_value: u64,
    raw_min_non_zero_value: u64,
    total_count: u64,
    normalizing_index_offset: i32,
    counts: BackingArray<T>,
}

// read methods
impl<T: Counter> Histogram<T> {
    pub(crate) fn settings(&self) -> &HistogramSettings {
        &self.settings
    }

    pub fn get_count_at_index(&self, index: u32) -> Option<&T> {
        if index >= self.counts_array_length() {
            return None;
        }
        let normalized_index = self.normalize_index(index);
        Some(self.counts.get_unchecked(normalized_index))
    }

    pub(crate) fn unsafe_get_count_at_index(&self, index: u32) -> &T {
        let normalized_index = self.normalize_index(index);
        self.counts.get_unchecked(normalized_index)
    }

    pub fn get_total_count(&self) -> u64 {
        self.total_count
    }

    pub fn counts_array_length(&self) -> u32 {
        self.counts.length()
    }

    #[inline(always)]
    fn normalize_index(&self, index: u32) -> u32 {
        util::normalize_index(index, self.normalizing_index_offset, self.counts_array_length())
    }

    #[inline(always)]
    fn unsafe_get_count_at_normalized_index(&self, index: u32) -> &T {
        self.counts.get_unchecked(index)
    }

    #[inline(always)]
    pub fn counts_array_index(&self, value: u64) -> u32 {
        self.settings.counts_array_index(value)
    }

    pub fn supports_auto_resize(&self) -> bool {
        true
    }

    pub fn hash_code(&self) -> i64 {
        use crate::core::util::hashing::*;
        let mut h = 0_i64;
        add_mix32(&mut h, self.settings.unit_magnitude);
        add_mix32(&mut h, self.settings.number_of_significant_value_digits);
        add_mix64(&mut h, self.total_count);
        add_mix64(&mut h, self.raw_max_value);
        add_mix64(&mut h, self.raw_min_non_zero_value);
        h += h << 3;
        h ^= h >> 11;
        h += h << 15;
        h
    }

    pub fn equals(&self, other: &Histogram<T>) -> bool {
        if std::ptr::eq(self, other) {
            return true;
        }
        if !(self.settings.equals(other.settings())) {
            return false;
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
        let self_len = self.counts_array_length();
        let other_len = other.counts_array_length();
        if self_len == other_len {
            for i in 0..self_len {
                if self.unsafe_get_count_at_index(i) != other.unsafe_get_count_at_index(i) {
                    return false;
                }
            }
        } else {
            let other_last = other_len - 1;
            for value in RecordedValuesIterator::new(self) {
                let mut other_index = other.counts_array_index(value.value_iterated_to);
                if other_index > other_last {
                    other_index = other_last;
                }
                let other_count = *other.unsafe_get_count_at_index(other_index);
                if value.count_at_value_iterated_to != other_count.as_u64() {
                    return false;
                }
            }
        }
        return true;
    }

    pub fn get_min_value(&self) -> u64 {
        if self.get_total_count() == 0 || *self.unsafe_get_count_at_index(0) != T::zero() {
            0
        } else {
            self.lowest_equivalent_value(self.raw_min_non_zero_value)
        }
    }

    pub fn get_max_value(&self) -> u64 {
        self.settings.get_max_value(self.raw_max_value)
    }

    pub fn get_min_non_zero_value(&self) -> u64 {
        self.settings
            .get_min_non_zero_value(self.raw_min_non_zero_value)
    }

    pub fn get_mean(&self) -> f64 {
        RecordedValuesIterator::get_mean_without_reset(&mut self.recorded_values())
    }

    pub fn get_std_deviation(&self) -> f64 {
        RecordedValuesIterator::get_std_deviation_without_reset(&mut self.recorded_values())
    }

    pub fn get_value_at_percentile(&self, percentile: f64) -> u64 {
        let one_below = util::next_below(percentile);
        // please define some max min for partialord values already, ffs
        let requested_percentile = if one_below > 100.0 {
            100.0
        } else if one_below < 0.0 {
            0.0
        } else {
            one_below
        };

        let fractional_count = (requested_percentile / 100.0) * self.total_count as f64;
        let mut count_at_percentile = fractional_count.ceil() as u64;

        // Make sure we at least reach the first recorded entry
        count_at_percentile = std::cmp::max(count_at_percentile, 1);

        let mut total_to_current_index: u64 = 0;
        for i in 0..self.counts_array_length() {
            total_to_current_index += self.unsafe_get_count_at_index(i).as_u64();
            if total_to_current_index >= count_at_percentile {
                let value_at_index = self.value_from_index(i);
                return if percentile == 0.0 {
                    self.lowest_equivalent_value(value_at_index)
                } else {
                    self.highest_equivalent_value(value_at_index)
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

    pub fn get_count_at_value(&self, value: u64) -> Option<T> {
        let idx = self.settings.counts_array_index(value);
        if idx < self.counts_array_length() {
            Some(*self.unsafe_get_count_at_index(idx))
        } else {
            None
        }
    }

    // settings proxy functions
    #[inline(always)]
    pub fn is_auto_resize(&self) -> bool {
        self.settings.auto_resize
    }
    pub fn get_lowest_discernable_value(&self) -> u64 {
        self.settings.lowest_discernible_value
    }
    pub fn get_highest_trackable_value(&self) -> u64 {
        self.settings.highest_trackable_value
    }
    pub fn get_number_of_significant_value_digits(&self) -> u32 {
        self.settings.number_of_significant_value_digits
    }
    pub(crate) fn integer_to_double_value_conversion_ratio(&self) -> f64 {
        self.settings.integer_to_double_value_conversion_ratio
    }
    pub(crate) fn double_to_integer_value_conversion_ratio(&self) -> f64 {
        self.settings.double_to_integer_value_conversion_ratio
    }
    pub(crate) fn set_integer_to_double_value_conversion_ratio(&mut self, ratio: f64) {
        self.settings.integer_to_double_value_conversion_ratio = ratio;
        self.settings.double_to_integer_value_conversion_ratio = 1.0 / ratio;
    }
    pub(crate) fn lowest_tracking_integer_value(&self) -> u64 {
        self.settings.sub_bucket_half_count as u64
    }
    pub fn lowest_equivalent_value(&self, value: u64) -> u64 {
        self.settings.lowest_equivalent_value(value)
    }
    pub fn highest_equivalent_value(&self, value: u64) -> u64 {
        self.settings.highest_equivalent_value(value)
    }
    pub fn median_equivalent_value(&self, value: u64) -> u64 {
        self.settings.median_equivalent_value(value)
    }
    pub fn next_non_equivalent_value(&self, value: u64) -> u64 {
        self.settings.next_non_equivalent_value(value)
    }
    pub fn size_of_equivalent_value_range(&self, value: u64) -> u64 {
        self.settings.size_of_equivalent_value_range(value)
    }
    pub fn values_are_equivalent(&self, v1: u64, v2: u64) -> bool {
        self.settings.values_are_equivalent(v1, v2)
    }
    pub fn value_from_index(&self, index: u32) -> u64 {
        self.settings.value_from_index(index)
    }
}

// write methods
impl<T: Counter> Histogram<T> {
    pub fn new(significant_value_digits: u8) -> Result<Histogram<T>, CreationError> {
        Histogram::<T>::with_sigvdig(significant_value_digits)
    }
    pub fn with_sigvdig(significant_value_digits: u8) -> Result<Histogram<T>, CreationError> {
        Histogram::<T>::with_high_sigvdig(2, significant_value_digits)
    }
    pub fn with_high_sigvdig(highest_trackable_value: u64, significant_value_digits: u8) -> Result<Histogram<T>, CreationError> {
        Histogram::<T>::with_low_high_sigvdig(1, highest_trackable_value, significant_value_digits)
    }
    pub fn with_low_high_sigvdig(
        lowest_discernible_value: u64,
        highest_trackable_value: u64,
        significant_value_digits: u8,
    ) -> Result<Histogram<T>, CreationError> {
        let settings = HistogramSettings::new(
            lowest_discernible_value,
            highest_trackable_value,
            significant_value_digits,
        )?;
        let counts_array_length = settings.counts_array_length;
        Ok(Histogram {
            meta_data: HistogramMetaData::new(),
            settings,
            raw_max_value: ORIGINAL_MAX,
            raw_min_non_zero_value: ORIGINAL_MIN,
            total_count: 0,
            normalizing_index_offset: 0,
            counts: BackingArray::new(counts_array_length),
        })
    }

    fn add_to_count_at_index(&mut self, idx: u32, count: T) {
        let normalized_index = self.normalize_index(idx);
        *self.counts.get_unchecked_mut(normalized_index) += count;
    }

    #[inline(always)]
    fn set_count_at_index(&mut self, idx: u32, count: T) {
        let normalized_index = self.normalize_index(idx);
        *self.counts.get_unchecked_mut(normalized_index) = count;
    }

    #[inline(always)]
    fn set_count_at_normalized_index(&mut self, idx: u32, count: T) {
        *self.counts.get_unchecked_mut(idx) = count;
    }

    fn update_max_value(&mut self, value: u64) {
        let internal_value = value | self.settings.unit_magnitude_mask;
        if internal_value > self.raw_max_value {
            self.raw_max_value = internal_value;
        }
    }

    fn reset_max_value(&mut self, max_value: u64) {
        self.raw_max_value = max_value | self.settings.unit_magnitude_mask;
    }

    fn update_min_non_zero_value(&mut self, value: u64) {
        if value <= self.settings.unit_magnitude_mask {
            return; // Unit-equivalent to 0.
        }

        let internal_value = value & !self.settings.unit_magnitude_mask;
        if internal_value < self.raw_min_non_zero_value {
            self.raw_min_non_zero_value = internal_value;
        }
    }

    fn reset_min_non_zero_value(&mut self, min_non_zero_value: u64) {
        let internal_value = min_non_zero_value & !self.settings.unit_magnitude_mask;
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

    pub(crate) fn shift_values_left(&mut self, number_of_binary_orders_of_magnitude: u32) -> Result<(), ShiftError> {
        if number_of_binary_orders_of_magnitude == 0 {
            return Ok(());
        }
        if self.total_count == self.unsafe_get_count_at_index(0).as_u64() {
            return Ok(());
        }

        let shift_amount = number_of_binary_orders_of_magnitude << self.settings.sub_bucket_half_count_magnitude;
        let max_value_index = self.counts_array_index(self.get_max_value());
        if max_value_index >= (self.counts_array_length() - shift_amount) {
            return Err(ShiftError::Overflow);
        }

        let max_before = self.raw_max_value;
        let min_before = self.raw_min_non_zero_value;
        self.raw_max_value = ORIGINAL_MAX;
        self.raw_min_non_zero_value = ORIGINAL_MIN;

        let lowest_half_bucket_populated =
            min_before < ((self.settings.sub_bucket_half_count as u64) << self.settings.unit_magnitude);
        self.shift_normalizing_index_by_offset(shift_amount as i32, lowest_half_bucket_populated)?;

        self.update_min_and_max(max_before << number_of_binary_orders_of_magnitude);
        if min_before != ORIGINAL_MIN {
            self.update_min_and_max(min_before << number_of_binary_orders_of_magnitude);
        }
        Ok(())
    }

    pub(crate) fn shift_values_right(&mut self, number_of_binary_orders_of_magnitude: u32) -> Result<(), ShiftError> {
        if number_of_binary_orders_of_magnitude == 0 {
            return Ok(());
        }
        if self.total_count == self.unsafe_get_count_at_index(0).as_u64() {
            return Ok(());
        }

        let shift_amount = self.settings.sub_bucket_half_count * number_of_binary_orders_of_magnitude;
        let min_non_zero_value_index = self.counts_array_index(self.get_min_non_zero_value());
        if min_non_zero_value_index < shift_amount + self.settings.sub_bucket_half_count {
            return Err(ShiftError::Underflow);
        }

        let max_before = self.raw_max_value;
        let min_before = self.raw_min_non_zero_value;
        self.raw_max_value = ORIGINAL_MAX;
        self.raw_min_non_zero_value = ORIGINAL_MIN;

        self.shift_normalizing_index_by_offset(-(shift_amount as i32), false)?;

        self.update_min_and_max(max_before >> number_of_binary_orders_of_magnitude);
        if min_before != ORIGINAL_MIN {
            self.update_min_and_max(min_before >> number_of_binary_orders_of_magnitude);
        }
        Ok(())
    }

    fn shift_normalizing_index_by_offset(
        &mut self,
        offset_to_add: i32,
        lowest_half_bucket_populated: bool,
    ) -> Result<(), ShiftError> {
        let zero_value_count = *self.unsafe_get_count_at_index(0);
        self.set_count_at_index(0, T::zero());
        let pre_shift_zero_index =
            util::normalize_index(0, self.normalizing_index_offset, self.counts_array_length());

        self.normalizing_index_offset += offset_to_add;

        if lowest_half_bucket_populated {
            if offset_to_add <= 0 {
                return Err(ShiftError::Underflow);
            }
            self.shift_lowest_half_bucket_contents_left(offset_to_add as u32, pre_shift_zero_index);
        }

        self.set_count_at_index(0, zero_value_count);
        Ok(())
    }

    fn shift_lowest_half_bucket_contents_left(&mut self, shift_amount: u32, pre_shift_zero_index: u32) {
        let number_of_binary_orders_of_magnitude = shift_amount >> self.settings.sub_bucket_half_count_magnitude;
        for from_index in 1..self.settings.sub_bucket_half_count {
            let to_value = self.value_from_index(from_index) << number_of_binary_orders_of_magnitude;
            let to_index = self.counts_array_index(to_value);
            let from_normalized_index = from_index + pre_shift_zero_index;
            let count_at_from_index = *self.unsafe_get_count_at_normalized_index(from_normalized_index);
            self.set_count_at_index(to_index, count_at_from_index);
            self.set_count_at_normalized_index(from_normalized_index, T::zero());
        }
    }

    pub fn set_auto_resize(&mut self, auto_resize: bool) {
        self.settings.auto_resize = auto_resize;
    }

    #[inline(always)]
    pub fn record_value(&mut self, value: u64) -> Result<(), RecordError> {
        self.record_value_with_count(value, T::one())
    }

    #[inline(always)]
    pub fn record_value_with_count(&mut self, value: u64, count: T) -> Result<(), RecordError> {
        self.record_count_at_value(count, value)
    }

    #[inline(always)]
    fn record_count_at_value(&mut self, count: T, value: u64) -> Result<(), RecordError> {
        let idx = self.settings.counts_array_index(value);

        if idx < self.counts.length() {
            self.add_to_count_at_index(idx, count);
            self.update_min_and_max(value);
            self.total_count += count.as_u64();
            Ok(())
        } else if !self.is_auto_resize() {
            let last_idx = self.counts.length() - 1;
            self.add_to_count_at_index(last_idx, count);
            self.update_min_and_max(value);
            self.total_count += count.as_u64();
            Ok(())
        } else {
            self.resize_and_record(value, idx, count)
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

    pub fn add<B: Borrow<Histogram<T>>>(&mut self, other_histogram: B) -> Result<(), RecordError> {
        let other_histogram = other_histogram.borrow();

        let highest_recordable_value = self.highest_equivalent_value(self.value_from_index(self.last_index()));

        let other_max_value = other_histogram.get_max_value();

        if highest_recordable_value < other_max_value {
            if !self.is_auto_resize() {
                return Err(RecordError::ValueOutOfRangeResizeDisabled);
            }
            self.resize(other_max_value)
                .map_err(|e| RecordError::ResizeFailed(e))?;
        }

        if self.settings
            .is_add_compatible_with(other_histogram.settings())
            && self.normalizing_index_offset == other_histogram.normalizing_index_offset
        {
            // Counts arrays are of the same length and meaning,
            // so we can just iterate and add directly:
            let mut observed_other_total_count: u64 = 0;
            for i in 0..other_histogram.counts_array_length() {
                let other_count = *other_histogram.unsafe_get_count_at_index(i);
                if other_count != T::zero() {
                    self.add_to_count_at_index(i, other_count);
                    observed_other_total_count += other_count.as_u64();
                }
            }

            self.total_count += observed_other_total_count;
            self.update_max_value(other_histogram.get_max_value());
            self.update_min_non_zero_value(other_histogram.get_min_non_zero_value());
        } else {
            // Arrays are not a direct match so we can't just stream through and add them.
            // Instead, go through the array and add each non-zero value found at it's proper value:

            // Do max value first, to avoid max value updates on each iteration:
            let other_max_index = other_histogram.counts_array_index(other_histogram.get_max_value());
            let other_count = *other_histogram.unsafe_get_count_at_index(other_max_index);
            self.record_value_with_count(
                other_histogram.value_from_index(other_max_index),
                other_count,
            )?;

            // Record the remaining values, up to but not including the max value:
            for i in 0..other_max_index {
                let other_count = *other_histogram.unsafe_get_count_at_index(i);
                if other_count != T::zero() {
                    let other_value = other_histogram.value_from_index(i);
                    self.record_value_with_count(other_value, other_count)?;
                }
            }
        }
        Ok(())
    }

    pub fn subtract<B: Borrow<Histogram<T>>>(&mut self, other_histogram: B) -> Result<(), SubtractionError> {
        let other_histogram = other_histogram.borrow();

        // make sure we can take the values in source
        let highest_recordable_value = self.highest_equivalent_value(self.value_from_index(self.last_index()));
        let other_max_value = self.highest_equivalent_value(other_histogram.get_max_value());
        if highest_recordable_value < other_max_value {
            return Err(SubtractionError::ValueOutOfRange);
        }

        for i in 0..other_histogram.counts_array_length() {
            let other_count = *other_histogram.unsafe_get_count_at_index(i);
            if other_count != T::zero() {
                let other_value = other_histogram.value_from_index(i);
                if self.get_count_at_value(other_value).unwrap() < other_count {
                    return Err(SubtractionError::CountExceededAtValue);
                }
                let idx = self.settings.counts_array_index(other_value);
                let normalized_index = self.normalize_index(idx);
                let count = self.counts.get_unchecked_mut(normalized_index);
                *count -= other_count;
            }
        }

        self.establish_internal_tracking_values();

        Ok(())
    }

    fn establish_internal_tracking_values(&mut self) {
        self.reset_max_value(ORIGINAL_MAX);
        self.reset_min_non_zero_value(ORIGINAL_MIN);
        let counts_array_length = self.counts_array_length();
        let (new_max, new_min, new_total) = util::recalculate_internal_tracking_values(self, counts_array_length);
        new_max.map(|mi| {
            let new_max = self.highest_equivalent_value(self.value_from_index(mi));
            self.update_max_value(new_max);
        });
        new_min.map(|mi| {
            let new_min = self.value_from_index(mi);
            self.update_min_non_zero_value(new_min);
        });
        self.total_count = new_total;
    }

    pub fn reset(&mut self) {
        self.counts.clear();
        self.reset_max_value(ORIGINAL_MAX);
        self.reset_min_non_zero_value(ORIGINAL_MIN);
        self.total_count = 0;
        self.normalizing_index_offset = 0;
        self.meta_data.clear();
    }

    #[inline(never)]
    pub fn resize(&mut self, value: u64) -> Result<(), CreationError> {
        let old_length = self.counts_array_length();
        let new_length = self.settings.resize(value)?;
        if new_length <= old_length {
            return Ok(());
        }
        let counts_delta = new_length - old_length;
        let old_zero_index = util::normalize_index(0, self.normalizing_index_offset, old_length);
        self.counts.grow(new_length);
        if old_zero_index != 0 {
            for i in (old_zero_index..old_length).rev() {
                let value = *self.counts.get_unchecked(i);
                *self.counts.get_unchecked_mut(i + counts_delta) = value;
            }
            let new_zero_index = old_zero_index + counts_delta;
            for i in old_zero_index..new_zero_index {
                *self.counts.get_unchecked_mut(i) = T::zero();
            }
        }
        Ok(())
    }

    #[inline(never)]
    fn resize_and_record(&mut self, value: u64, idx: u32, count: T) -> Result<(), RecordError> {
        self.resize(value)
            .map(|_| {
                self.add_to_count_at_index(idx, count);

                self.update_min_and_max(value);
                self.total_count += count.as_u64();
            })
            .map_err(|e| RecordError::ResizeFailed(e))
    }

    pub fn percentiles(&self, percentile_ticks_per_half_distance: u32) -> PercentileIterator<Self> {
        PercentileIterator::new(self, percentile_ticks_per_half_distance)
    }

    pub fn linear_bucket_values(&self, value_units_per_bucket: u64) -> LinearIterator<Self> {
        LinearIterator::new(self, value_units_per_bucket)
    }

    pub fn logarithmic_bucket_values(&self, value_units_in_first_bucket: u64, log_base: f64) -> LogarithmicIterator<Self> {
        LogarithmicIterator::new(self, value_units_in_first_bucket, log_base)
    }

    pub fn all_values(&self) -> AllValuesIterator<Self> {
        AllValuesIterator::new(self)
    }

    pub fn recorded_values(&self) -> RecordedValuesIterator<Self> {
        RecordedValuesIterator::new(self)
    }
}

impl<T: Counter> ConstructableHistogram for Histogram<T> {
    fn new(lowest_discernible_value: u64, highest_trackable_value: u64, significant_value_digits: u8) -> Result<Self, CreationError> {
        Histogram::<T>::with_low_high_sigvdig(
            lowest_discernible_value,
            highest_trackable_value,
            significant_value_digits,
        )
    }

    fn establish_internal_tracking_values(&mut self) {
        Histogram::<T>::establish_internal_tracking_values(self)
    }
}

impl<T: Counter> ReadableHistogram for Histogram<T> {
    fn settings(&self) -> &HistogramSettings {
        &self.settings
    }
    #[inline(always)]
    fn array_length(&self) -> u32 {
        self.counts_array_length()
    }
    #[inline(always)]
    fn get_total_count(&self) -> u64 {
        Histogram::<T>::get_total_count(self)
    }
    #[inline(always)]
    fn unsafe_get_count_at_index(&self, idx: u32) -> u64 {
        Histogram::<T>::unsafe_get_count_at_index(self, idx).as_u64()
    }
    fn get_max_value(&self) -> u64 {
        Histogram::<T>::get_max_value(self)
    }

    fn meta_data(&self) -> &HistogramMetaData { &self.meta_data }
}

impl<T: Counter> Histogram<T> {
    pub fn get_counts_slice<'a>(&'a self, length: u32) -> Option<&'a [T]> {
        self.counts.get_slice(length)
    }
    pub fn get_counts_slice_mut<'a>(&'a mut self, length: u32) -> Option<&'a mut [T]> {
        self.counts.get_slice_mut(length)
    }
    #[inline(always)]
    pub fn saturating_counts_array_index(&self, value: u64) -> u32 {
        let idx = self.counts_array_index(value);
        let max_idx = self.counts_array_length() - 1;
        if idx > max_idx {
            max_idx
        } else {
            idx
        }
    }

    #[inline(always)]
    pub fn last_index(&self) -> u32 {
        self.counts_array_length() - 1
    }
}

impl<T: Counter> PartialEq for Histogram<T> {
    fn eq(&self, other: &Self) -> bool {
        self.equals(other)
    }
}
