use crate::core::constants::*;
use crate::core::counter::Counter;
use crate::core::meta_data::HistogramMetaData;
use crate::core::*;
use crate::iteration::*;
use std::marker::PhantomData;

const DEFAULT_SIGNIFICANT_VALUE_DIGITS: u8 = 3;

#[repr(C)]
pub struct FixedInlineHistogramWithCounter<T, const N: usize> {
    pub meta_data: HistogramMetaData,
    layout: HistogramLayout,
    storage_metadata: HistogramStorageMetadata,
    raw_max_value: u64,
    raw_min_non_zero_value: u64,
    total_count: u64,
    normalizing_index_offset: i32,
    integer_to_double_value_conversion_ratio: f64,
    double_to_integer_value_conversion_ratio: f64,
    counts: [T; N],
}

/// Fixed-range integer histogram with `u64` counters stored directly inline.
///
/// `N` is the maximum number of count slots available in the embedded array.
/// The builder rejects configurations whose required HdrHistogram counts array
/// does not fit in `N`.
pub type FixedInlineHistogram<const N: usize> = FixedInlineHistogramWithCounter<u64, N>;

/// Builder for fixed-range inline integer histograms.
///
/// Builders default to three decimal significant digits. The histogram does not
/// support auto-resize; choose `N` large enough for the configured highest
/// trackable value.
pub struct FixedInlineHistogramBuilder<T, const N: usize> {
    lowest_discernible_value: u64,
    highest_trackable_value: u64,
    significant_value_digits: u8,
    _counter: PhantomData<T>,
}

impl<T: Counter, const N: usize> FixedInlineHistogramBuilder<T, N> {
    pub fn new() -> Self {
        FixedInlineHistogramBuilder {
            lowest_discernible_value: 1,
            highest_trackable_value: 2,
            significant_value_digits: DEFAULT_SIGNIFICANT_VALUE_DIGITS,
            _counter: PhantomData,
        }
    }

    /// Set the number of decimal significant digits retained by the histogram.
    ///
    /// The default is `3`, which gives roughly three significant decimal digits
    /// of precision.
    pub fn significant_digits(mut self, significant_value_digits: u8) -> Self {
        self.significant_value_digits = significant_value_digits;
        self
    }

    pub fn lowest_discernible_value(mut self, lowest_discernible_value: u64) -> Self {
        self.lowest_discernible_value = lowest_discernible_value;
        self
    }

    pub fn highest_trackable_value(mut self, highest_trackable_value: u64) -> Self {
        self.highest_trackable_value = highest_trackable_value;
        self
    }

    pub fn build(self) -> Result<FixedInlineHistogramWithCounter<T, N>, CreationError> {
        FixedInlineHistogramWithCounter::<T, N>::with_low_high_sigvdig(
            self.lowest_discernible_value,
            self.highest_trackable_value,
            self.significant_value_digits,
        )
    }
}

impl<T: Counter, const N: usize> FixedInlineHistogramWithCounter<T, N> {
    /// Create a builder for an inline fixed-range integer histogram.
    pub fn builder() -> FixedInlineHistogramBuilder<T, N> {
        FixedInlineHistogramBuilder::new()
    }

    pub(crate) fn with_low_high_sigvdig(
        lowest_discernible_value: u64,
        highest_trackable_value: u64,
        significant_value_digits: u8,
    ) -> Result<Self, CreationError> {
        let layout = HistogramLayout::new(lowest_discernible_value, highest_trackable_value, significant_value_digits)?;
        let storage_metadata = layout.initial_metadata_for_highest(highest_trackable_value)?;
        if storage_metadata.counts_array_length as usize > N {
            return Err(CreationError::RequiresExcessiveArrayLen);
        }

        Ok(FixedInlineHistogramWithCounter {
            meta_data: HistogramMetaData::new(),
            layout,
            storage_metadata,
            raw_max_value: ORIGINAL_MAX,
            raw_min_non_zero_value: ORIGINAL_MIN,
            total_count: 0,
            normalizing_index_offset: 0,
            integer_to_double_value_conversion_ratio: 1.0,
            double_to_integer_value_conversion_ratio: 1.0,
            counts: [T::zero(); N],
        })
    }

    /// Return a snapshot of the settings that define this histogram's precision
    /// and fixed trackable range.
    pub fn settings(&self) -> HistogramSettings {
        self.layout.settings_snapshot(self.storage_metadata, false)
    }

    pub fn get_count_at_index(&self, index: u32) -> Option<&T> {
        if index >= self.counts_array_length() {
            return None;
        }
        let normalized_index = self.normalize_index(index);
        Some(self.unsafe_get_count_at_normalized_index(normalized_index))
    }

    pub(crate) fn unsafe_get_count_at_index(&self, index: u32) -> &T {
        let normalized_index = self.normalize_index(index);
        self.unsafe_get_count_at_normalized_index(normalized_index)
    }

    #[inline(always)]
    fn unsafe_get_count_at_normalized_index(&self, index: u32) -> &T {
        unsafe { self.counts.get_unchecked(index as usize) }
    }

    #[inline(always)]
    fn unsafe_get_count_at_normalized_index_mut(&mut self, index: u32) -> &mut T {
        unsafe { self.counts.get_unchecked_mut(index as usize) }
    }

    pub fn get_total_count(&self) -> u64 {
        self.total_count
    }

    pub fn counts_array_length(&self) -> u32 {
        self.storage_metadata.counts_array_length
    }

    pub(crate) fn normalizing_index_offset(&self) -> i32 {
        self.normalizing_index_offset
    }

    #[inline(always)]
    fn normalize_index(&self, index: u32) -> u32 {
        util::normalize_index(index, self.normalizing_index_offset, self.counts_array_length())
    }

    #[inline(always)]
    pub fn counts_array_index(&self, value: u64) -> u32 {
        self.layout.counts_array_index(value)
    }

    pub fn get_count_at_value(&self, value: u64) -> Option<T> {
        let idx = self.layout.counts_array_index(value);
        if idx < self.counts_array_length() {
            Some(*self.unsafe_get_count_at_index(idx))
        } else {
            None
        }
    }

    pub fn supports_auto_resize(&self) -> bool {
        false
    }

    #[inline(always)]
    pub fn is_auto_resize(&self) -> bool {
        false
    }

    pub fn get_min_value(&self) -> u64 {
        if self.get_total_count() == 0 || *self.unsafe_get_count_at_index(0) != T::zero() {
            0
        } else {
            self.lowest_equivalent_value(self.raw_min_non_zero_value)
        }
    }

    pub fn get_max_value(&self) -> u64 {
        self.layout.get_max_value(self.raw_max_value)
    }

    pub fn get_min_non_zero_value(&self) -> u64 {
        self.layout.get_min_non_zero_value(self.raw_min_non_zero_value)
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

    pub fn get_lowest_discernible_value(&self) -> u64 {
        self.layout.lowest_discernible_value
    }

    pub fn get_highest_trackable_value(&self) -> u64 {
        self.storage_metadata.highest_trackable_value
    }

    pub fn get_number_of_significant_value_digits(&self) -> u32 {
        self.layout.number_of_significant_value_digits
    }

    pub(crate) fn integer_to_double_value_conversion_ratio(&self) -> f64 {
        self.integer_to_double_value_conversion_ratio
    }

    pub(crate) fn set_integer_to_double_value_conversion_ratio(&mut self, ratio: f64) {
        self.integer_to_double_value_conversion_ratio = ratio;
        self.double_to_integer_value_conversion_ratio = self.layout.double_to_integer_value_conversion_ratio_for_integer_to_double(ratio);
    }

    pub(crate) fn lowest_tracking_integer_value(&self) -> u64 {
        self.layout.sub_bucket_half_count as u64
    }

    pub fn lowest_equivalent_value(&self, value: u64) -> u64 {
        self.layout.lowest_equivalent_value(value)
    }

    pub fn highest_equivalent_value(&self, value: u64) -> u64 {
        self.layout.highest_equivalent_value(value)
    }

    pub fn median_equivalent_value(&self, value: u64) -> u64 {
        self.layout.median_equivalent_value(value)
    }

    pub fn next_non_equivalent_value(&self, value: u64) -> u64 {
        self.layout.next_non_equivalent_value(value)
    }

    pub fn size_of_equivalent_value_range(&self, value: u64) -> u64 {
        self.layout.size_of_equivalent_value_range(value)
    }

    pub fn values_are_equivalent(&self, v1: u64, v2: u64) -> bool {
        self.layout.values_are_equivalent(v1, v2)
    }

    pub fn value_from_index(&self, index: u32) -> u64 {
        self.layout.value_from_index(index)
    }

    #[inline(always)]
    fn add_to_count_at_index(&mut self, idx: u32, count: T) {
        let normalized_index = self.normalize_index(idx);
        *self.unsafe_get_count_at_normalized_index_mut(normalized_index) += count;
    }

    fn update_max_value(&mut self, value: u64) {
        let internal_value = value | self.layout.unit_magnitude_mask;
        if internal_value > self.raw_max_value {
            self.raw_max_value = internal_value;
        }
    }

    fn reset_max_value(&mut self, max_value: u64) {
        self.raw_max_value = max_value | self.layout.unit_magnitude_mask;
    }

    fn update_min_non_zero_value(&mut self, value: u64) {
        if value <= self.layout.unit_magnitude_mask {
            return;
        }

        let internal_value = value & !self.layout.unit_magnitude_mask;
        if internal_value < self.raw_min_non_zero_value {
            self.raw_min_non_zero_value = internal_value;
        }
    }

    fn reset_min_non_zero_value(&mut self, min_non_zero_value: u64) {
        let internal_value = min_non_zero_value & !self.layout.unit_magnitude_mask;
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
        let idx = self.layout.counts_array_index(value);

        if idx < self.counts_array_length() {
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
        let idx = self.layout.counts_array_index(value);

        if idx < self.counts_array_length() {
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

    fn establish_internal_tracking_values(&mut self) {
        self.reset_max_value(ORIGINAL_MAX);
        self.reset_min_non_zero_value(ORIGINAL_MIN);
        let counts_array_length = self.counts_array_length();
        let (new_max, new_min, new_total) = util::recalculate_internal_tracking_values(self, counts_array_length);
        if let Some(mi) = new_max {
            let new_max = self.highest_equivalent_value(self.value_from_index(mi));
            self.update_max_value(new_max);
        }
        if let Some(mi) = new_min {
            let new_min = self.value_from_index(mi);
            self.update_min_non_zero_value(new_min);
        }
        self.total_count = new_total;
    }

    pub fn reset(&mut self) {
        for value in &mut self.counts[..self.storage_metadata.counts_array_length as usize] {
            *value = T::zero();
        }
        self.reset_max_value(ORIGINAL_MAX);
        self.reset_min_non_zero_value(ORIGINAL_MIN);
        self.total_count = 0;
        self.normalizing_index_offset = 0;
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
        if length <= self.storage_metadata.counts_array_length as usize {
            Some(&self.counts[..length])
        } else {
            None
        }
    }

    pub(crate) fn get_counts_slice_mut(&mut self, length: u32) -> Option<&mut [T]> {
        let length = length as usize;
        if length <= self.storage_metadata.counts_array_length as usize {
            Some(&mut self.counts[..length])
        } else {
            None
        }
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

impl<T: Counter, const N: usize> crate::core::readable_histogram::sealed::Sealed for FixedInlineHistogramWithCounter<T, N> {}

impl<T: Counter, const N: usize> ReadableHistogram for FixedInlineHistogramWithCounter<T, N> {
    fn settings(&self) -> HistogramSettings {
        self.settings()
    }

    #[inline(always)]
    fn array_length(&self) -> u32 {
        self.counts_array_length()
    }

    #[inline(always)]
    fn get_total_count(&self) -> u64 {
        FixedInlineHistogramWithCounter::<T, N>::get_total_count(self)
    }

    #[inline(always)]
    fn unsafe_get_count_at_index(&self, idx: u32) -> u64 {
        FixedInlineHistogramWithCounter::<T, N>::unsafe_get_count_at_index(self, idx).as_u64()
    }

    fn get_max_value(&self) -> u64 {
        FixedInlineHistogramWithCounter::<T, N>::get_max_value(self)
    }

    fn meta_data(&self) -> &HistogramMetaData {
        &self.meta_data
    }

    fn integer_to_double_value_conversion_ratio(&self) -> f64 {
        FixedInlineHistogramWithCounter::<T, N>::integer_to_double_value_conversion_ratio(self)
    }

    fn normalizing_index_offset(&self) -> i32 {
        FixedInlineHistogramWithCounter::<T, N>::normalizing_index_offset(self)
    }
}

impl<T: Counter, const N: usize> IterableHistogram for FixedInlineHistogramWithCounter<T, N> {}

impl<T: Counter, const N: usize> EncodableHistogram for FixedInlineHistogramWithCounter<T, N> {}

impl<T: Counter, const N: usize> ConstructableHistogram for FixedInlineHistogramWithCounter<T, N> {
    fn new(lowest_discernible_value: u64, highest_trackable_value: u64, significant_value_digits: u8) -> Result<Self, CreationError> {
        FixedInlineHistogramWithCounter::<T, N>::with_low_high_sigvdig(
            lowest_discernible_value,
            highest_trackable_value,
            significant_value_digits,
        )
    }

    fn establish_internal_tracking_values(&mut self) {
        FixedInlineHistogramWithCounter::<T, N>::establish_internal_tracking_values(self)
    }
}
