use crate::core::{DoubleCreationError, OverflowPolicy, RecordError, SaturateOnOverflow, ThrowOnOverflow};
use crate::iteration::RecordedValuesIterator;
use crate::st::Histogram;
use std::marker::PhantomData;
use std::sync::OnceLock;

fn highest_allowed_value_ever() -> f64 {
    static HIGHEST: OnceLock<f64> = OnceLock::new();
    *HIGHEST.get_or_init(|| {
        let mut value = 1.0;
        while value < f64::MAX / 4.0 {
            value *= 2.0;
        }
        value
    })
}

fn ulp(value: f64) -> f64 {
    if value.is_nan() {
        return f64::NAN;
    }
    if value.is_infinite() {
        return f64::INFINITY;
    }
    let bits = value.to_bits();
    if value >= 0.0 {
        f64::from_bits(bits + 1) - value
    } else {
        value - f64::from_bits(bits - 1)
    }
}

fn find_containing_binary_order_of_magnitude_long(long_number: u64) -> u32 {
    64 - long_number.leading_zeros()
}

fn find_containing_binary_order_of_magnitude_double(double_number: f64) -> u32 {
    let long_number = double_number.ceil() as u64;
    find_containing_binary_order_of_magnitude_long(long_number)
}

fn find_capped_containing_binary_order_of_magnitude(double_number: f64, configured_ratio: u64) -> u32 {
    if double_number > configured_ratio as f64 {
        return (configured_ratio as f64).log2().floor() as u32;
    }
    if double_number > (1_u64 << 50) as f64 {
        return 50;
    }
    find_containing_binary_order_of_magnitude_double(double_number)
}

fn derive_internal_highest_to_lowest_value_ratio(external_ratio: u64) -> u64 {
    1_u64 << (find_containing_binary_order_of_magnitude_long(external_ratio) + 1)
}

fn sub_bucket_half_count_for_sig_digits(significant_value_digits: u8) -> u64 {
    let largest_value_with_single_unit_resolution = 2_u64 * 10_u64.pow(significant_value_digits as u32);
    let sub_bucket_count = largest_value_with_single_unit_resolution.next_power_of_two();
    sub_bucket_count / 2
}

fn derive_integer_value_range(external_ratio: u64, significant_value_digits: u8) -> Option<u64> {
    let internal_ratio = derive_internal_highest_to_lowest_value_ratio(external_ratio);
    let lowest_tracking_integer_value = sub_bucket_half_count_for_sig_digits(significant_value_digits);
    lowest_tracking_integer_value.checked_mul(internal_ratio)
}

pub struct DoubleHistogramImpl<P: OverflowPolicy> {
    integer_histogram: Histogram<u64>,
    configured_highest_to_lowest_value_ratio: u64,
    current_lowest_value_in_auto_range: f64,
    current_highest_value_limit_in_auto_range: f64,
    auto_resize: bool,
    _policy: PhantomData<P>,
}

pub type DoubleHistogram = DoubleHistogramImpl<ThrowOnOverflow>;
pub type SaturatingDoubleHistogram = DoubleHistogramImpl<SaturateOnOverflow>;

impl<P: OverflowPolicy> DoubleHistogramImpl<P> {
    pub fn new(number_of_significant_value_digits: u8) -> Result<Self, DoubleCreationError> {
        let mut histogram = Self::with_highest_to_lowest_value_ratio(2, number_of_significant_value_digits)?;
        histogram.set_auto_resize(true);
        Ok(histogram)
    }

    pub fn with_highest_to_lowest_value_ratio(
        highest_to_lowest_value_ratio: u64,
        number_of_significant_value_digits: u8,
    ) -> Result<Self, DoubleCreationError> {
        if highest_to_lowest_value_ratio < 2 {
            return Err(DoubleCreationError::HighestToLowestValueRatioTooSmall);
        }
        if number_of_significant_value_digits > 5 {
            return Err(DoubleCreationError::SignificantValueDigitsExceedsMax);
        }
        let sig_digits_factor = 10_u128.pow(number_of_significant_value_digits as u32);
        let ratio_check = (highest_to_lowest_value_ratio as u128) * sig_digits_factor;
        if ratio_check >= (1_u128 << 61) {
            return Err(DoubleCreationError::HighestToLowestValueRatioTooLarge);
        }

        let integer_value_range =
            derive_integer_value_range(highest_to_lowest_value_ratio, number_of_significant_value_digits)
                .ok_or(DoubleCreationError::HighestToLowestValueRatioTooLarge)?;
        let highest_trackable_value = integer_value_range - 1;
        let integer_histogram = Histogram::<u64>::with_low_high_sigvdig(
            1,
            highest_trackable_value,
            number_of_significant_value_digits,
        )
        .map_err(DoubleCreationError::Internal)?;
        let mut histogram = DoubleHistogramImpl {
            integer_histogram,
            configured_highest_to_lowest_value_ratio: highest_to_lowest_value_ratio,
            current_lowest_value_in_auto_range: 0.0,
            current_highest_value_limit_in_auto_range: 0.0,
            auto_resize: false,
            _policy: PhantomData,
        };
        let initial_lowest_value_in_auto_range = 2.0_f64.powi(800);
        histogram.init(highest_to_lowest_value_ratio, initial_lowest_value_in_auto_range);
        Ok(histogram)
    }

    pub fn record_value(&mut self, value: f64) -> Result<(), RecordError> {
        self.record_value_with_count(value, 1)
    }

    pub fn record_value_with_count(&mut self, value: f64, count: u64) -> Result<(), RecordError> {
        self.record_count_at_value(count, value)
    }

    pub fn record_value_with_expected_interval(
        &mut self,
        value: f64,
        expected_interval_between_value_samples: f64,
    ) -> Result<(), RecordError> {
        self.record_value_with_count_and_expected_interval(value, 1, expected_interval_between_value_samples)
    }

    pub fn get_count_at_value(&self, value: f64) -> u64 {
        let integer_value = self.to_integer_value_clamped(value);
        let idx = self.integer_histogram.saturating_counts_array_index(integer_value);
        *self.integer_histogram.unsafe_get_count_at_index(idx)
    }

    pub fn get_total_count(&self) -> u64 {
        self.integer_histogram.get_total_count()
    }

    pub fn get_min_value(&self) -> f64 {
        self.integer_histogram.get_min_value() as f64 * self.get_integer_to_double_value_conversion_ratio()
    }

    pub fn get_max_value(&self) -> f64 {
        self.highest_equivalent_value(
            self.integer_histogram.get_max_value() as f64 * self.get_integer_to_double_value_conversion_ratio(),
        )
    }

    pub fn get_mean(&self) -> f64 {
        self.integer_histogram.get_mean() * self.get_integer_to_double_value_conversion_ratio()
    }

    pub fn get_std_deviation(&self) -> f64 {
        self.integer_histogram.get_std_deviation() * self.get_integer_to_double_value_conversion_ratio()
    }

    pub fn get_value_at_percentile(&self, percentile: f64) -> f64 {
        self.integer_histogram.get_value_at_percentile(percentile) as f64
            * self.get_integer_to_double_value_conversion_ratio()
    }

    pub fn get_percentile_at_or_below_value(&self, value: f64) -> f64 {
        let integer_value = self.to_integer_value_clamped(value);
        self.integer_histogram
            .get_percentile_at_or_below_value(integer_value)
    }

    pub fn size_of_equivalent_value_range(&self, value: f64) -> f64 {
        self.integer_histogram
            .size_of_equivalent_value_range(self.to_integer_value_clamped(value)) as f64
            * self.get_integer_to_double_value_conversion_ratio()
    }

    pub fn lowest_equivalent_value(&self, value: f64) -> f64 {
        self.integer_histogram
            .lowest_equivalent_value(self.to_integer_value_clamped(value)) as f64
            * self.get_integer_to_double_value_conversion_ratio()
    }

    pub fn highest_equivalent_value(&self, value: f64) -> f64 {
        let next_non_equivalent_value = self.next_non_equivalent_value(value);
        let mut highest_equivalent_value = next_non_equivalent_value - (2.0 * ulp(next_non_equivalent_value));
        while highest_equivalent_value + ulp(highest_equivalent_value) < next_non_equivalent_value {
            highest_equivalent_value += ulp(highest_equivalent_value);
        }
        highest_equivalent_value
    }

    pub fn median_equivalent_value(&self, value: f64) -> f64 {
        self.integer_histogram
            .median_equivalent_value(self.to_integer_value_clamped(value)) as f64
            * self.get_integer_to_double_value_conversion_ratio()
    }

    pub fn values_are_equivalent(&self, value1: f64, value2: f64) -> bool {
        self.lowest_equivalent_value(value1) == self.lowest_equivalent_value(value2)
    }

    pub fn get_current_lowest_trackable_non_zero_value(&self) -> f64 {
        self.current_lowest_value_in_auto_range
    }

    pub fn get_current_highest_trackable_value(&self) -> f64 {
        self.current_highest_value_limit_in_auto_range
    }

    pub fn get_highest_to_lowest_value_ratio(&self) -> u64 {
        self.configured_highest_to_lowest_value_ratio
    }

    pub fn get_number_of_significant_value_digits(&self) -> u8 {
        self.integer_histogram.get_number_of_significant_value_digits() as u8
    }

    pub(crate) fn bucket_count(&self) -> u32 {
        self.integer_histogram.settings().bucket_count
    }

    pub(crate) fn counts_array_length(&self) -> u32 {
        self.integer_histogram.settings().counts_array_length
    }

    pub fn set_auto_resize(&mut self, auto_resize: bool) {
        self.auto_resize = auto_resize;
    }

    pub fn is_auto_resize(&self) -> bool {
        self.auto_resize
    }

    pub fn reset(&mut self) {
        self.integer_histogram.reset();
        let initial_lowest_value_in_auto_range = 2.0_f64.powi(800);
        let configured_ratio = self.configured_highest_to_lowest_value_ratio;
        self.init(configured_ratio, initial_lowest_value_in_auto_range);
    }

    pub fn add(&mut self, other: &Self) -> Result<(), RecordError> {
        let other_ratio = other.get_integer_to_double_value_conversion_ratio();
        for value in RecordedValuesIterator::new(&other.integer_histogram) {
            let double_value = value.value_iterated_to as f64 * other_ratio;
            self.record_value_with_count(double_value, value.count_at_value_iterated_to)?;
        }
        Ok(())
    }

    pub fn copy_corrected_for_coordinated_omission(
        &self,
        expected_interval_between_value_samples: f64,
    ) -> Result<Self, RecordError> {
        let mut target = DoubleHistogramImpl::with_highest_to_lowest_value_ratio(
            self.configured_highest_to_lowest_value_ratio,
            self.get_number_of_significant_value_digits(),
        )?;
        target.set_trackable_value_range(
            self.current_lowest_value_in_auto_range,
            self.current_highest_value_limit_in_auto_range,
        );
        target.add_while_correcting_for_coordinated_omission(self, expected_interval_between_value_samples)?;
        Ok(target)
    }

    fn init(&mut self, configured_highest_to_lowest_value_ratio: u64, lowest_trackable_unit_value: f64) {
        self.configured_highest_to_lowest_value_ratio = configured_highest_to_lowest_value_ratio;
        let internal_ratio = derive_internal_highest_to_lowest_value_ratio(configured_highest_to_lowest_value_ratio);
        let highest_value_limit = lowest_trackable_unit_value * internal_ratio as f64;
        self.set_trackable_value_range(lowest_trackable_unit_value, highest_value_limit);
    }

    fn set_trackable_value_range(&mut self, lowest_value_in_auto_range: f64, highest_value_in_auto_range: f64) {
        self.current_lowest_value_in_auto_range = lowest_value_in_auto_range;
        self.current_highest_value_limit_in_auto_range = highest_value_in_auto_range;
        let ratio = lowest_value_in_auto_range / self.integer_histogram.lowest_tracking_integer_value() as f64;
        self.integer_histogram.set_integer_to_double_value_conversion_ratio(ratio);
    }

    fn get_integer_to_double_value_conversion_ratio(&self) -> f64 {
        self.integer_histogram.integer_to_double_value_conversion_ratio()
    }

    fn get_double_to_integer_value_conversion_ratio(&self) -> f64 {
        self.integer_histogram.double_to_integer_value_conversion_ratio()
    }

    fn to_integer_value(&self, value: f64) -> Result<u64, RecordError> {
        if !value.is_finite() || value < 0.0 {
            return Err(RecordError::ValueOutOfRangeResizeDisabled);
        }
        let scaled = value * self.get_double_to_integer_value_conversion_ratio();
        if scaled > u64::MAX as f64 {
            return Err(RecordError::ValueOutOfRangeResizeDisabled);
        }
        Ok(scaled as u64)
    }

    fn to_integer_value_clamped(&self, value: f64) -> u64 {
        if !value.is_finite() || value <= 0.0 {
            return 0;
        }
        let scaled = value * self.get_double_to_integer_value_conversion_ratio();
        if scaled > u64::MAX as f64 {
            return u64::MAX;
        }
        scaled as u64
    }

    fn next_non_equivalent_value(&self, value: f64) -> f64 {
        self.integer_histogram
            .next_non_equivalent_value(self.to_integer_value_clamped(value)) as f64
            * self.get_integer_to_double_value_conversion_ratio()
    }

    fn record_value_with_count_and_expected_interval(
        &mut self,
        value: f64,
        count: u64,
        expected_interval_between_value_samples: f64,
    ) -> Result<(), RecordError> {
        self.record_count_at_value(count, value)?;
        if expected_interval_between_value_samples <= 0.0 {
            return Ok(());
        }
        let mut missing_value = value - expected_interval_between_value_samples;
        while missing_value >= expected_interval_between_value_samples {
            self.record_count_at_value(count, missing_value)?;
            missing_value -= expected_interval_between_value_samples;
        }
        Ok(())
    }

    fn record_count_at_value(&mut self, count: u64, value: f64) -> Result<(), RecordError> {
        if !value.is_finite() {
            return Err(RecordError::ValueOutOfRangeResizeDisabled);
        }
        if value == 0.0 {
            return self.integer_histogram.record_value_with_count_strict(0, count);
        }
        if value < 0.0 {
            return Err(RecordError::ValueOutOfRangeResizeDisabled);
        }

        if value < self.current_lowest_value_in_auto_range || value >= self.current_highest_value_limit_in_auto_range {
            if let Err(err) = self.auto_adjust_range_for_value(value) {
                if P::SATURATE {
                    if value < self.current_lowest_value_in_auto_range {
                        let clamped_value = self.current_lowest_value_in_auto_range;
                        let integer_value = self.to_integer_value(clamped_value)?;
                        return self.integer_histogram.record_value_with_count(integer_value, count);
                    }
                    let integer_value = self.to_integer_value(value)?;
                    return self.integer_histogram.record_value_with_count(integer_value, count);
                }
                return Err(err);
            }
        }

        let integer_value = self.to_integer_value(value)?;
        match self
            .integer_histogram
            .record_value_with_count_strict(integer_value, count)
        {
            Ok(()) => Ok(()),
            Err(err) => {
                if P::SATURATE {
                    self.integer_histogram.record_value_with_count(integer_value, count)
                } else {
                    Err(err)
                }
            }
        }
    }

    fn auto_adjust_range_for_value(&mut self, value: f64) -> Result<(), RecordError> {
        if value == 0.0 {
            return Ok(());
        }
        if value < 0.0 {
            return Err(RecordError::ValueOutOfRangeResizeDisabled);
        }
        if value < self.current_lowest_value_in_auto_range {
            loop {
                let shift_amount = find_capped_containing_binary_order_of_magnitude(
                    (self.current_lowest_value_in_auto_range / value).ceil() - 1.0,
                    self.configured_highest_to_lowest_value_ratio,
                );
                self.shift_covered_range_to_the_right(shift_amount)?;
                if value >= self.current_lowest_value_in_auto_range {
                    break;
                }
            }
        } else if value >= self.current_highest_value_limit_in_auto_range {
            if value > highest_allowed_value_ever() {
                return Err(RecordError::ValueOutOfRangeResizeDisabled);
            }
            loop {
                let shift_amount = find_capped_containing_binary_order_of_magnitude(
                    ((value + ulp(value)) / self.current_highest_value_limit_in_auto_range).ceil() - 1.0,
                    self.configured_highest_to_lowest_value_ratio,
                );
                self.shift_covered_range_to_the_left(shift_amount)?;
                if value < self.current_highest_value_limit_in_auto_range {
                    break;
                }
            }
        }
        Ok(())
    }

    fn shift_covered_range_to_the_right(&mut self, number_of_binary_orders_of_magnitude: u32) -> Result<(), RecordError> {
        let mut new_lowest = self.current_lowest_value_in_auto_range;
        let mut new_highest = self.current_highest_value_limit_in_auto_range;

        let shift_multiplier = 1.0 / (1_u64 << number_of_binary_orders_of_magnitude) as f64;
        self.current_highest_value_limit_in_auto_range *= shift_multiplier;

        let result = (|| {
            if self.integer_histogram.get_total_count()
                > *self.integer_histogram.unsafe_get_count_at_index(0)
            {
                if self
                    .integer_histogram
                    .shift_values_left(number_of_binary_orders_of_magnitude)
                    .is_err()
                {
                    self.handle_shift_values_exception(number_of_binary_orders_of_magnitude)?;
                    new_highest /= shift_multiplier;
                    self.integer_histogram
                        .shift_values_left(number_of_binary_orders_of_magnitude)
                        .map_err(|_| RecordError::ValueOutOfRangeResizeDisabled)?;
                }
            }
            new_lowest *= shift_multiplier;
            new_highest *= shift_multiplier;
            Ok(())
        })();

        self.set_trackable_value_range(new_lowest, new_highest);
        result
    }

    fn shift_covered_range_to_the_left(&mut self, number_of_binary_orders_of_magnitude: u32) -> Result<(), RecordError> {
        let mut new_lowest = self.current_lowest_value_in_auto_range;
        let mut new_highest = self.current_highest_value_limit_in_auto_range;

        let shift_multiplier = 1.0 * (1_u64 << number_of_binary_orders_of_magnitude) as f64;
        self.current_lowest_value_in_auto_range *= shift_multiplier;

        let result = (|| {
            if self.integer_histogram.get_total_count()
                > *self.integer_histogram.unsafe_get_count_at_index(0)
            {
                match self
                    .integer_histogram
                    .shift_values_right(number_of_binary_orders_of_magnitude)
                {
                    Ok(()) => {
                        new_lowest *= shift_multiplier;
                        new_highest *= shift_multiplier;
                    }
                    Err(_) => {
                        self.handle_shift_values_exception(number_of_binary_orders_of_magnitude)?;
                        new_lowest /= shift_multiplier;
                    }
                }
            }
            new_lowest *= shift_multiplier;
            new_highest *= shift_multiplier;
            Ok(())
        })();

        self.set_trackable_value_range(new_lowest, new_highest);
        result
    }

    fn handle_shift_values_exception(
        &mut self,
        number_of_binary_orders_of_magnitude: u32,
    ) -> Result<(), RecordError> {
        if !self.auto_resize {
            return Err(RecordError::ValueOutOfRangeResizeDisabled);
        }
        let highest_trackable_value = self.integer_histogram.get_highest_trackable_value();
        let current_containing_order = find_containing_binary_order_of_magnitude_long(highest_trackable_value);
        let new_containing_order = current_containing_order + number_of_binary_orders_of_magnitude;
        if new_containing_order > 63 {
            return Err(RecordError::ValueOutOfRangeResizeDisabled);
        }
        let new_highest_trackable_value = (1_u64 << new_containing_order) - 1;
        self.integer_histogram
            .resize(new_highest_trackable_value)
            .map_err(RecordError::ResizeFailed)?;
        self.configured_highest_to_lowest_value_ratio <<= number_of_binary_orders_of_magnitude;
        Ok(())
    }

    fn add_while_correcting_for_coordinated_omission(
        &mut self,
        other: &Self,
        expected_interval_between_value_samples: f64,
    ) -> Result<(), RecordError> {
        let other_ratio = other.get_integer_to_double_value_conversion_ratio();
        for value in RecordedValuesIterator::new(&other.integer_histogram) {
            let double_value = value.value_iterated_to as f64 * other_ratio;
            self.record_value_with_count_and_expected_interval(
                double_value,
                value.count_at_value_iterated_to,
                expected_interval_between_value_samples,
            )?;
        }
        Ok(())
    }
}
