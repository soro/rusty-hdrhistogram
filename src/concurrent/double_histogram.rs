use crate::concurrent::recordable_histogram::RecordableHistogram;
use crate::concurrent::{ResizableConcurrentHistogram, ResizableConcurrentReadView, ResizableStructuralMutation};
use crate::core::util;
use crate::core::{
    DoubleCreationError, EncodableHistogram, HistogramMetaData, HistogramSettings, OverflowPolicy, ReadableHistogram, RecordError,
    SaturateOnOverflow, ThrowOnOverflow,
};
use crate::iteration::{
    DoubleAllValuesIterator, DoubleLinearIterator, DoubleLogarithmicIterator, DoublePercentileIterator, DoubleRecordedValuesIterator,
    RecordedValuesIterator,
};
use parking_lot::Mutex;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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

pub struct ConcurrentDoubleHistogramImpl<P: OverflowPolicy> {
    integer_histogram: ResizableConcurrentHistogram,
    configured_highest_to_lowest_value_ratio: AtomicU64,
    current_lowest_value_in_auto_range: AtomicU64,
    current_highest_value_limit_in_auto_range: AtomicU64,
    auto_resize: AtomicBool,
    range_shift_in_progress: AtomicBool,
    range_generation: AtomicU64,
    range_lock: Mutex<()>,
    _policy: PhantomData<P>,
}

/// A structurally stable read view of a [`ConcurrentDoubleHistogramImpl`].
///
/// The view captures double range metadata together with an underlying integer
/// read view from one structural epoch. Ordinary recording can still update
/// count cells while the view is alive, so aggregate fields such as total count,
/// min, and max are not guaranteed to be a frozen point-in-time count snapshot.
/// Use recorder samples/snapshots when that stronger guarantee is needed.
pub struct ConcurrentDoubleReadView<'a> {
    integer_view: ResizableConcurrentReadView<'a>,
    configured_highest_to_lowest_value_ratio: u64,
    current_lowest_value_in_auto_range: f64,
    current_highest_value_limit_in_auto_range: f64,
    auto_resize: bool,
}

/// A read-only sampled concurrent double histogram.
///
/// Recorder samples return this wrapper so sampled interval data can be queried
/// and iterated without exposing the underlying concurrent histogram's
/// writer-side mutation methods.
pub struct ConcurrentDoubleSnapshot<'a, P: OverflowPolicy> {
    histogram: &'a ConcurrentDoubleHistogramImpl<P>,
}

pub type ConcurrentDoubleHistogram = ConcurrentDoubleHistogramImpl<ThrowOnOverflow>;
pub type SaturatingConcurrentDoubleHistogram = ConcurrentDoubleHistogramImpl<SaturateOnOverflow>;

struct RangeShiftGate<'a> {
    range_shift_in_progress: &'a AtomicBool,
}

enum SingleValueOutOfRangeResult {
    Recorded,
    Retry,
    Continue,
}

impl Drop for RangeShiftGate<'_> {
    fn drop(&mut self) {
        self.range_shift_in_progress.store(false, Ordering::SeqCst);
    }
}

impl ConcurrentDoubleReadView<'_> {
    pub fn meta_data(&self) -> &HistogramMetaData {
        self.integer_view.meta_data()
    }

    pub fn get_count_at_value(&self, value: f64) -> u64 {
        let integer_value = to_integer_value_clamped_for_histogram(self, value);
        let idx = self.settings().counts_array_index(integer_value).min(self.array_length() - 1);
        self.unsafe_get_count_at_index(idx)
    }

    pub fn get_total_count(&self) -> u64 {
        self.integer_view.get_total_count()
    }

    pub fn get_min_value(&self) -> f64 {
        if self.get_total_count() == 0 || self.integer_view.unsafe_get_count_at_index(0) != 0 {
            0.0
        } else {
            self.settings().lowest_equivalent_value(self.integer_view.get_min_non_zero_value()) as f64
                * self.integer_to_double_value_conversion_ratio()
        }
    }

    pub fn get_max_value(&self) -> f64 {
        highest_equivalent_value_for_histogram(
            self,
            self.integer_view.get_max_value() as f64 * self.integer_to_double_value_conversion_ratio(),
        )
    }

    pub fn try_get_mean(&self) -> Result<f64, crate::iteration::IterationError> {
        let mut iterator = RecordedValuesIterator::from_readable(self);
        RecordedValuesIterator::try_get_mean_without_reset(&mut iterator).map(|mean| mean * self.integer_to_double_value_conversion_ratio())
    }

    pub fn try_get_std_deviation(&self) -> Result<f64, crate::iteration::IterationError> {
        let mut iterator = RecordedValuesIterator::from_readable(self);
        RecordedValuesIterator::try_get_std_deviation_without_reset(&mut iterator)
            .map(|std_deviation| std_deviation * self.integer_to_double_value_conversion_ratio())
    }

    pub fn percentiles(&self, percentile_ticks_per_half_distance: u32) -> DoublePercentileIterator<&'_ Self> {
        DoublePercentileIterator::from_readable(self, percentile_ticks_per_half_distance)
    }

    pub fn linear_bucket_values(&self, value_units_per_bucket: f64) -> DoubleLinearIterator<&'_ Self> {
        DoubleLinearIterator::from_readable(self, value_units_per_bucket)
    }

    pub fn logarithmic_bucket_values(&self, value_units_in_first_bucket: f64, log_base: f64) -> DoubleLogarithmicIterator<&'_ Self> {
        DoubleLogarithmicIterator::from_readable(self, value_units_in_first_bucket, log_base)
    }

    pub fn all_values(&self) -> DoubleAllValuesIterator<&'_ Self> {
        DoubleAllValuesIterator::from_readable(self)
    }

    pub fn recorded_values(&self) -> DoubleRecordedValuesIterator<&'_ Self> {
        DoubleRecordedValuesIterator::from_readable(self)
    }

    pub fn get_value_at_percentile(&self, percentile: f64) -> f64 {
        get_value_at_percentile_for_histogram(self, percentile) as f64 * self.integer_to_double_value_conversion_ratio()
    }

    pub fn get_percentile_at_or_below_value(&self, value: f64) -> f64 {
        let integer_value = to_integer_value_clamped_for_histogram(self, value);
        get_percentile_at_or_below_value_for_histogram(self, integer_value)
    }

    pub fn size_of_equivalent_value_range(&self, value: f64) -> f64 {
        self.settings()
            .size_of_equivalent_value_range(to_integer_value_clamped_for_histogram(self, value)) as f64
            * self.integer_to_double_value_conversion_ratio()
    }

    pub fn lowest_equivalent_value(&self, value: f64) -> f64 {
        lowest_equivalent_value_for_histogram(self, value)
    }

    pub fn highest_equivalent_value(&self, value: f64) -> f64 {
        highest_equivalent_value_for_histogram(self, value)
    }

    pub fn median_equivalent_value(&self, value: f64) -> f64 {
        self.settings()
            .median_equivalent_value(to_integer_value_clamped_for_histogram(self, value)) as f64
            * self.integer_to_double_value_conversion_ratio()
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
        self.settings().number_of_significant_value_digits as u8
    }

    pub fn is_auto_resize(&self) -> bool {
        self.auto_resize
    }
}

impl<'a, P: OverflowPolicy> ConcurrentDoubleSnapshot<'a, P> {
    pub(crate) fn new(histogram: &'a ConcurrentDoubleHistogramImpl<P>) -> Self {
        ConcurrentDoubleSnapshot { histogram }
    }

    pub fn read_view(&self) -> ConcurrentDoubleReadView<'_> {
        self.histogram.read_view()
    }

    pub fn meta_data(&self) -> &HistogramMetaData {
        self.histogram.meta_data()
    }

    pub fn settings(&self) -> HistogramSettings {
        self.read_view().settings()
    }

    pub fn get_count_at_value(&self, value: f64) -> u64 {
        self.histogram.get_count_at_value(value)
    }

    pub fn get_total_count(&self) -> u64 {
        self.histogram.get_total_count()
    }

    pub fn get_min_value(&self) -> f64 {
        self.histogram.get_min_value()
    }

    pub fn get_max_value(&self) -> f64 {
        self.histogram.get_max_value()
    }

    pub fn try_get_mean(&self) -> Result<f64, crate::iteration::IterationError> {
        self.histogram.try_get_mean()
    }

    pub fn try_get_std_deviation(&self) -> Result<f64, crate::iteration::IterationError> {
        self.histogram.try_get_std_deviation()
    }

    pub fn get_value_at_percentile(&self, percentile: f64) -> f64 {
        self.histogram.get_value_at_percentile(percentile)
    }

    pub fn get_percentile_at_or_below_value(&self, value: f64) -> f64 {
        self.histogram.get_percentile_at_or_below_value(value)
    }

    pub fn size_of_equivalent_value_range(&self, value: f64) -> f64 {
        self.histogram.size_of_equivalent_value_range(value)
    }

    pub fn lowest_equivalent_value(&self, value: f64) -> f64 {
        self.histogram.lowest_equivalent_value(value)
    }

    pub fn highest_equivalent_value(&self, value: f64) -> f64 {
        self.histogram.highest_equivalent_value(value)
    }

    pub fn median_equivalent_value(&self, value: f64) -> f64 {
        self.histogram.median_equivalent_value(value)
    }

    pub fn values_are_equivalent(&self, value1: f64, value2: f64) -> bool {
        self.histogram.values_are_equivalent(value1, value2)
    }

    pub fn get_current_lowest_trackable_non_zero_value(&self) -> f64 {
        self.histogram.get_current_lowest_trackable_non_zero_value()
    }

    pub fn get_current_highest_trackable_value(&self) -> f64 {
        self.histogram.get_current_highest_trackable_value()
    }

    pub fn get_highest_to_lowest_value_ratio(&self) -> u64 {
        self.histogram.get_highest_to_lowest_value_ratio()
    }

    pub fn get_number_of_significant_value_digits(&self) -> u8 {
        self.histogram.get_number_of_significant_value_digits()
    }

    pub fn is_auto_resize(&self) -> bool {
        self.histogram.is_auto_resize()
    }

    pub fn percentiles(&self, percentile_ticks_per_half_distance: u32) -> DoublePercentileIterator<ConcurrentDoubleReadView<'_>> {
        self.histogram.percentiles_snapshot(percentile_ticks_per_half_distance)
    }

    pub fn linear_bucket_values(&self, value_units_per_bucket: f64) -> DoubleLinearIterator<ConcurrentDoubleReadView<'_>> {
        self.histogram.linear_bucket_values_snapshot(value_units_per_bucket)
    }

    pub fn logarithmic_bucket_values(
        &self,
        value_units_in_first_bucket: f64,
        log_base: f64,
    ) -> DoubleLogarithmicIterator<ConcurrentDoubleReadView<'_>> {
        self.histogram
            .logarithmic_bucket_values_snapshot(value_units_in_first_bucket, log_base)
    }

    pub fn all_values(&self) -> DoubleAllValuesIterator<ConcurrentDoubleReadView<'_>> {
        self.histogram.all_values_snapshot()
    }

    pub fn recorded_values(&self) -> DoubleRecordedValuesIterator<ConcurrentDoubleReadView<'_>> {
        self.histogram.recorded_values_snapshot()
    }
}

impl ReadableHistogram for ConcurrentDoubleReadView<'_> {
    fn settings(&self) -> HistogramSettings {
        self.integer_view.settings()
    }

    fn array_length(&self) -> u32 {
        self.integer_view.array_length()
    }

    fn get_total_count(&self) -> u64 {
        self.integer_view.get_total_count()
    }

    fn current_total_count(&self) -> u64 {
        self.integer_view.current_total_count()
    }

    fn unsafe_get_count_at_index(&self, idx: u32) -> u64 {
        self.integer_view.unsafe_get_count_at_index(idx)
    }

    fn get_max_value(&self) -> u64 {
        self.integer_view.get_max_value()
    }

    fn meta_data(&self) -> &HistogramMetaData {
        self.integer_view.meta_data()
    }

    fn integer_to_double_value_conversion_ratio(&self) -> f64 {
        self.integer_view.integer_to_double_value_conversion_ratio()
    }

    fn normalizing_index_offset(&self) -> i32 {
        self.integer_view.normalizing_index_offset()
    }
}

impl EncodableHistogram for ConcurrentDoubleReadView<'_> {}

impl<P: OverflowPolicy> ConcurrentDoubleHistogramImpl<P> {
    pub fn new(number_of_significant_value_digits: u8) -> Result<Self, DoubleCreationError> {
        let histogram = Self::with_highest_to_lowest_value_ratio(2, number_of_significant_value_digits)?;
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

        let integer_value_range = derive_integer_value_range(highest_to_lowest_value_ratio, number_of_significant_value_digits)
            .ok_or(DoubleCreationError::HighestToLowestValueRatioTooLarge)?;
        let highest_trackable_value = integer_value_range - 1;
        let integer_histogram =
            ResizableConcurrentHistogram::with_low_high_sigvdig(1, highest_trackable_value, number_of_significant_value_digits)
                .map_err(DoubleCreationError::Internal)?;

        let histogram = ConcurrentDoubleHistogramImpl {
            integer_histogram,
            configured_highest_to_lowest_value_ratio: AtomicU64::new(highest_to_lowest_value_ratio),
            current_lowest_value_in_auto_range: AtomicU64::new(0.0_f64.to_bits()),
            current_highest_value_limit_in_auto_range: AtomicU64::new(0.0_f64.to_bits()),
            auto_resize: AtomicBool::new(false),
            range_shift_in_progress: AtomicBool::new(false),
            range_generation: AtomicU64::new(0),
            range_lock: Mutex::new(()),
            _policy: PhantomData,
        };
        let initial_lowest_value_in_auto_range = 2.0_f64.powi(800);
        histogram.init(highest_to_lowest_value_ratio, initial_lowest_value_in_auto_range);
        Ok(histogram)
    }

    pub fn record_value(&self, value: f64) -> Result<(), RecordError> {
        self.record_single_value(value)
    }

    pub fn record_value_with_count(&self, value: f64, count: u64) -> Result<(), RecordError> {
        self.record_count_at_value(count, value)
    }

    pub fn record_value_with_expected_interval(&self, value: f64, expected_interval_between_value_samples: f64) -> Result<(), RecordError> {
        self.record_value_with_count_and_expected_interval(value, 1, expected_interval_between_value_samples)
    }

    /// Capture a structurally stable double read view.
    ///
    /// This preserves range metadata and backing-array mapping for encoding and
    /// point queries. It does not prevent ordinary concurrent recordings from
    /// updating count cells while the view is used.
    pub fn read_view(&self) -> ConcurrentDoubleReadView<'_> {
        let _range_guard = self.range_lock.lock();
        let integer_view = self.integer_histogram.read_view();
        ConcurrentDoubleReadView {
            integer_view,
            configured_highest_to_lowest_value_ratio: self.configured_highest_to_lowest_value_ratio.load(Ordering::Relaxed),
            current_lowest_value_in_auto_range: self.current_lowest_value_in_auto_range(),
            current_highest_value_limit_in_auto_range: self.current_highest_value_limit_in_auto_range(),
            auto_resize: self.auto_resize.load(Ordering::Relaxed),
        }
    }

    pub fn get_count_at_value(&self, value: f64) -> u64 {
        self.read_view().get_count_at_value(value)
    }

    pub fn get_total_count(&self) -> u64 {
        self.read_view().get_total_count()
    }

    pub fn get_min_value(&self) -> f64 {
        self.read_view().get_min_value()
    }

    pub fn get_max_value(&self) -> f64 {
        self.read_view().get_max_value()
    }

    pub fn try_get_mean(&self) -> Result<f64, crate::iteration::IterationError> {
        self.read_view().try_get_mean()
    }

    pub fn try_get_std_deviation(&self) -> Result<f64, crate::iteration::IterationError> {
        self.read_view().try_get_std_deviation()
    }

    pub fn get_value_at_percentile(&self, percentile: f64) -> f64 {
        self.read_view().get_value_at_percentile(percentile)
    }

    pub fn get_percentile_at_or_below_value(&self, value: f64) -> f64 {
        self.read_view().get_percentile_at_or_below_value(value)
    }

    pub fn size_of_equivalent_value_range(&self, value: f64) -> f64 {
        self.read_view().size_of_equivalent_value_range(value)
    }

    pub fn lowest_equivalent_value(&self, value: f64) -> f64 {
        self.read_view().lowest_equivalent_value(value)
    }

    pub fn highest_equivalent_value(&self, value: f64) -> f64 {
        self.read_view().highest_equivalent_value(value)
    }

    pub fn median_equivalent_value(&self, value: f64) -> f64 {
        self.read_view().median_equivalent_value(value)
    }

    pub fn values_are_equivalent(&self, value1: f64, value2: f64) -> bool {
        self.read_view().values_are_equivalent(value1, value2)
    }

    pub fn get_current_lowest_trackable_non_zero_value(&self) -> f64 {
        self.current_lowest_value_in_auto_range()
    }

    pub fn get_current_highest_trackable_value(&self) -> f64 {
        self.current_highest_value_limit_in_auto_range()
    }

    pub fn get_highest_to_lowest_value_ratio(&self) -> u64 {
        self.configured_highest_to_lowest_value_ratio.load(Ordering::Relaxed)
    }

    pub fn get_number_of_significant_value_digits(&self) -> u8 {
        self.integer_histogram.settings().number_of_significant_value_digits as u8
    }

    pub(crate) fn bucket_count(&self) -> u32 {
        self.integer_histogram.settings().bucket_count
    }

    pub(crate) fn counts_array_length(&self) -> u32 {
        self.integer_histogram.settings().counts_array_length
    }

    pub(crate) fn meta_data_mut(&mut self) -> &mut HistogramMetaData {
        self.integer_histogram.meta_data_mut()
    }

    pub fn meta_data(&self) -> &HistogramMetaData {
        self.integer_histogram.meta_data()
    }

    pub fn set_auto_resize(&self, auto_resize: bool) {
        self.auto_resize.store(auto_resize, Ordering::Relaxed);
    }

    pub fn is_auto_resize(&self) -> bool {
        self.auto_resize.load(Ordering::Relaxed)
    }

    pub fn reset(&self) {
        let _range_guard = self.range_lock.lock();
        let mut mutation = self.integer_histogram.begin_structural_mutation();
        let _gate = self.close_range_shift_gate();
        mutation.flip();
        unsafe {
            mutation.clear_counts();
        }
        let configured_ratio = self.configured_highest_to_lowest_value_ratio.load(Ordering::Relaxed);
        let initial_lowest_value_in_auto_range = 2.0_f64.powi(800);
        self.configured_highest_to_lowest_value_ratio
            .store(configured_ratio, Ordering::Relaxed);
        let internal_ratio = derive_internal_highest_to_lowest_value_ratio(configured_ratio);
        let highest_value_limit = initial_lowest_value_in_auto_range * internal_ratio as f64;
        self.set_trackable_value_range_with_mutation(&mut mutation, initial_lowest_value_in_auto_range, highest_value_limit);
        self.publish_range_generation();
    }

    pub fn add(&self, other: &Self) -> Result<(), RecordError> {
        let other_view = other.read_view();
        let other_ratio = other_view.integer_to_double_value_conversion_ratio();
        let mut iterator = RecordedValuesIterator::from_readable(other_view);
        while let Some(value) = iterator.try_next().map_err(|_| RecordError::ConcurrentModification)? {
            let double_value = value.value_iterated_to as f64 * other_ratio;
            self.record_value_with_count(double_value, value.count_at_value_iterated_to)?;
        }
        Ok(())
    }

    pub fn copy_corrected_for_coordinated_omission(&self, expected_interval_between_value_samples: f64) -> Result<Self, RecordError> {
        let source_view = self.read_view();
        let source_settings = source_view.settings();
        let source_ratio = source_view.integer_to_double_value_conversion_ratio();

        let target = ConcurrentDoubleHistogramImpl::with_highest_to_lowest_value_ratio(
            source_view.get_highest_to_lowest_value_ratio(),
            source_settings.number_of_significant_value_digits as u8,
        )?;
        target.set_trackable_value_range(
            source_view.get_current_lowest_trackable_non_zero_value(),
            source_view.get_current_highest_trackable_value(),
        );
        target.add_view_while_correcting_for_coordinated_omission(source_view, source_ratio, expected_interval_between_value_samples)?;
        Ok(target)
    }

    fn init(&self, configured_highest_to_lowest_value_ratio: u64, lowest_trackable_unit_value: f64) {
        self.configured_highest_to_lowest_value_ratio
            .store(configured_highest_to_lowest_value_ratio, Ordering::Relaxed);
        let internal_ratio = derive_internal_highest_to_lowest_value_ratio(configured_highest_to_lowest_value_ratio);
        let highest_value_limit = lowest_trackable_unit_value * internal_ratio as f64;
        self.set_trackable_value_range(lowest_trackable_unit_value, highest_value_limit);
    }

    fn set_trackable_value_range(&self, lowest_value_in_auto_range: f64, highest_value_in_auto_range: f64) {
        self.set_trackable_value_range_fields(lowest_value_in_auto_range, highest_value_in_auto_range);
        let ratio = self.integer_to_double_value_conversion_ratio_for_lowest(lowest_value_in_auto_range);
        self.integer_histogram.set_integer_to_double_value_conversion_ratio(ratio);
    }

    fn set_trackable_value_range_with_mutation(
        &self,
        mutation: &mut ResizableStructuralMutation<'_>,
        lowest_value_in_auto_range: f64,
        highest_value_in_auto_range: f64,
    ) {
        self.set_trackable_value_range_fields(lowest_value_in_auto_range, highest_value_in_auto_range);
        let ratio = self.integer_to_double_value_conversion_ratio_for_lowest(lowest_value_in_auto_range);
        mutation.set_integer_to_double_value_conversion_ratio(ratio);
    }

    fn set_trackable_value_range_fields(&self, lowest_value_in_auto_range: f64, highest_value_in_auto_range: f64) {
        self.current_lowest_value_in_auto_range
            .store(lowest_value_in_auto_range.to_bits(), Ordering::Relaxed);
        self.current_highest_value_limit_in_auto_range
            .store(highest_value_in_auto_range.to_bits(), Ordering::Relaxed);
    }

    fn integer_to_double_value_conversion_ratio_for_lowest(&self, lowest_value_in_auto_range: f64) -> f64 {
        lowest_value_in_auto_range / self.lowest_tracking_integer_value() as f64
    }

    fn current_lowest_value_in_auto_range(&self) -> f64 {
        f64::from_bits(self.current_lowest_value_in_auto_range.load(Ordering::Relaxed))
    }

    fn current_highest_value_limit_in_auto_range(&self) -> f64 {
        f64::from_bits(self.current_highest_value_limit_in_auto_range.load(Ordering::Relaxed))
    }

    fn lowest_tracking_integer_value(&self) -> u64 {
        self.integer_histogram.lowest_tracking_integer_value()
    }

    fn integer_to_double_value_conversion_ratio(&self) -> f64 {
        self.integer_histogram.integer_to_double_value_conversion_ratio()
    }

    fn record_value_with_count_and_expected_interval(
        &self,
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

    #[inline(always)]
    fn record_single_value(&self, value: f64) -> Result<(), RecordError> {
        let mut throw_count = 0;
        'record: loop {
            let observed_range_generation = self.range_generation.load(Ordering::Acquire);
            let current_lowest = self.current_lowest_value_in_auto_range();
            let current_highest = self.current_highest_value_limit_in_auto_range();

            if !(value >= current_lowest && value < current_highest) {
                match self.record_single_value_out_of_range(value, current_lowest, observed_range_generation)? {
                    SingleValueOutOfRangeResult::Recorded => return Ok(()),
                    SingleValueOutOfRangeResult::Retry => {
                        std::hint::spin_loop();
                        continue 'record;
                    }
                    SingleValueOutOfRangeResult::Continue => {}
                }
            }

            let result = if P::SATURATE {
                self.integer_histogram.record_in_range_converted_double_value_saturating_guarded(
                    value,
                    &self.range_shift_in_progress,
                    &self.range_generation,
                    observed_range_generation,
                )
            } else {
                self.integer_histogram.record_in_range_converted_double_value_guarded(
                    value,
                    &self.range_shift_in_progress,
                    &self.range_generation,
                    observed_range_generation,
                )
            };

            match result {
                Ok(true) => return Ok(()),
                Ok(false) => std::hint::spin_loop(),
                Err(_) => {
                    throw_count += 1;
                    if throw_count > 64 {
                        return Err(RecordError::ValueOutOfRangeResizeDisabled);
                    }
                }
            }
        }
    }

    #[inline(never)]
    fn record_single_value_out_of_range(
        &self,
        value: f64,
        current_lowest: f64,
        observed_range_generation: u64,
    ) -> Result<SingleValueOutOfRangeResult, RecordError> {
        if !value.is_finite() {
            return Err(RecordError::ValueOutOfRangeResizeDisabled);
        }
        if value == 0.0 {
            return match self
                .integer_histogram
                .record_value_with_count_strict_guarded(0, 1, &self.range_shift_in_progress)?
            {
                true => Ok(SingleValueOutOfRangeResult::Recorded),
                false => Ok(SingleValueOutOfRangeResult::Retry),
            };
        }
        if value < 0.0 {
            return Err(RecordError::ValueOutOfRangeResizeDisabled);
        }

        if let Err(err) = self.auto_adjust_range_for_value(value) {
            if P::SATURATE {
                if value < current_lowest {
                    let clamped_value = current_lowest;
                    return match self.integer_histogram.record_in_range_converted_double_value_saturating_guarded(
                        clamped_value,
                        &self.range_shift_in_progress,
                        &self.range_generation,
                        observed_range_generation,
                    )? {
                        true => Ok(SingleValueOutOfRangeResult::Recorded),
                        false => Ok(SingleValueOutOfRangeResult::Retry),
                    };
                }
                return match self.integer_histogram.record_converted_double_value_with_count_saturating_guarded(
                    value,
                    1,
                    &self.range_shift_in_progress,
                    &self.range_generation,
                    observed_range_generation,
                )? {
                    true => Ok(SingleValueOutOfRangeResult::Recorded),
                    false => Ok(SingleValueOutOfRangeResult::Retry),
                };
            }
            return Err(err);
        }

        Ok(SingleValueOutOfRangeResult::Continue)
    }

    fn record_count_at_value(&self, count: u64, value: f64) -> Result<(), RecordError> {
        if !value.is_finite() {
            return Err(RecordError::ValueOutOfRangeResizeDisabled);
        }
        if value == 0.0 {
            loop {
                match self
                    .integer_histogram
                    .record_value_with_count_strict_guarded(0, count, &self.range_shift_in_progress)?
                {
                    true => return Ok(()),
                    false => std::hint::spin_loop(),
                }
            }
        }
        if value < 0.0 {
            return Err(RecordError::ValueOutOfRangeResizeDisabled);
        }

        let mut throw_count = 0;
        'record: loop {
            let observed_range_generation = self.range_generation.load(Ordering::Acquire);
            let current_lowest = self.current_lowest_value_in_auto_range();
            let current_highest = self.current_highest_value_limit_in_auto_range();

            if value < current_lowest || value >= current_highest {
                if let Err(err) = self.auto_adjust_range_for_value(value) {
                    if P::SATURATE {
                        if value < current_lowest {
                            let clamped_value = current_lowest;
                            loop {
                                match self.integer_histogram.record_converted_double_value_with_count_saturating_guarded(
                                    clamped_value,
                                    count,
                                    &self.range_shift_in_progress,
                                    &self.range_generation,
                                    observed_range_generation,
                                )? {
                                    true => return Ok(()),
                                    false => {
                                        std::hint::spin_loop();
                                        continue 'record;
                                    }
                                }
                            }
                        }
                        loop {
                            match self.integer_histogram.record_converted_double_value_with_count_saturating_guarded(
                                value,
                                count,
                                &self.range_shift_in_progress,
                                &self.range_generation,
                                observed_range_generation,
                            )? {
                                true => return Ok(()),
                                false => {
                                    std::hint::spin_loop();
                                    continue 'record;
                                }
                            }
                        }
                    }
                    return Err(err);
                }
            }

            match self.integer_histogram.record_converted_double_value_with_count_guarded(
                value,
                count,
                &self.range_shift_in_progress,
                &self.range_generation,
                observed_range_generation,
            ) {
                Ok(true) => return Ok(()),
                Ok(false) => std::hint::spin_loop(),
                Err(_) => {
                    throw_count += 1;
                    if throw_count > 64 {
                        return Err(RecordError::ValueOutOfRangeResizeDisabled);
                    }
                }
            }
        }
    }

    fn auto_adjust_range_for_value(&self, value: f64) -> Result<(), RecordError> {
        if value == 0.0 {
            return Ok(());
        }
        if value < 0.0 {
            return Err(RecordError::ValueOutOfRangeResizeDisabled);
        }
        let _guard = self.range_lock.lock();
        loop {
            let current_lowest = self.current_lowest_value_in_auto_range();
            let current_highest = self.current_highest_value_limit_in_auto_range();
            if value < current_lowest {
                let shift_amount = find_capped_containing_binary_order_of_magnitude(
                    (current_lowest / value).ceil() - 1.0,
                    self.configured_highest_to_lowest_value_ratio.load(Ordering::Relaxed),
                );
                self.shift_covered_range_to_the_right(shift_amount)?;
                continue;
            }
            if value >= current_highest {
                if value > highest_allowed_value_ever() {
                    return Err(RecordError::ValueOutOfRangeResizeDisabled);
                }
                let shift_amount = find_capped_containing_binary_order_of_magnitude(
                    ((value + ulp(value)) / current_highest).ceil() - 1.0,
                    self.configured_highest_to_lowest_value_ratio.load(Ordering::Relaxed),
                );
                self.shift_covered_range_to_the_left(shift_amount)?;
                continue;
            }
            break;
        }
        Ok(())
    }

    fn close_range_shift_gate(&self) -> RangeShiftGate<'_> {
        let was_closed = self.range_shift_in_progress.swap(true, Ordering::SeqCst);
        debug_assert!(!was_closed);
        RangeShiftGate {
            range_shift_in_progress: &self.range_shift_in_progress,
        }
    }

    fn publish_range_generation(&self) {
        self.range_generation.fetch_add(1, Ordering::Release);
    }

    fn shift_covered_range_to_the_right(&self, number_of_binary_orders_of_magnitude: u32) -> Result<(), RecordError> {
        let mut new_lowest = self.current_lowest_value_in_auto_range();
        let mut new_highest = self.current_highest_value_limit_in_auto_range();
        let original_lowest = new_lowest;
        let original_highest = new_highest;
        let shift_multiplier = 1.0 / (1_u64 << number_of_binary_orders_of_magnitude) as f64;

        let mut mutation = self.integer_histogram.begin_structural_mutation();
        let _gate = self.close_range_shift_gate();
        mutation.flip();

        let new_integer_to_double_value_conversion_ratio = mutation.integer_to_double_value_conversion_ratio() * shift_multiplier;
        self.current_highest_value_limit_in_auto_range
            .store((new_highest * shift_multiplier).to_bits(), Ordering::Relaxed);

        let result = (|| {
            if self.integer_histogram.get_total_count() > mutation.count_at_index(0)
                && mutation
                    .shift_values_left_with_conversion_ratio(
                        number_of_binary_orders_of_magnitude,
                        new_integer_to_double_value_conversion_ratio,
                    )
                    .is_err()
            {
                self.handle_shift_values_exception(&mut mutation, number_of_binary_orders_of_magnitude)?;
                new_highest /= shift_multiplier;
                mutation
                    .shift_values_left_with_conversion_ratio(
                        number_of_binary_orders_of_magnitude,
                        new_integer_to_double_value_conversion_ratio,
                    )
                    .map_err(|_| RecordError::ValueOutOfRangeResizeDisabled)?;
            }
            new_lowest *= shift_multiplier;
            new_highest *= shift_multiplier;
            Ok(())
        })();

        if result.is_ok() {
            self.set_trackable_value_range_with_mutation(&mut mutation, new_lowest, new_highest);
            self.publish_range_generation();
        } else {
            self.set_trackable_value_range_fields(original_lowest, original_highest);
        }
        result
    }

    fn shift_covered_range_to_the_left(&self, number_of_binary_orders_of_magnitude: u32) -> Result<(), RecordError> {
        let mut new_lowest = self.current_lowest_value_in_auto_range();
        let mut new_highest = self.current_highest_value_limit_in_auto_range();
        let original_lowest = new_lowest;
        let original_highest = new_highest;
        let shift_multiplier = 1.0 * (1_u64 << number_of_binary_orders_of_magnitude) as f64;

        let mut mutation = self.integer_histogram.begin_structural_mutation();
        let _gate = self.close_range_shift_gate();
        mutation.flip();

        let new_integer_to_double_value_conversion_ratio = mutation.integer_to_double_value_conversion_ratio() * shift_multiplier;
        self.current_lowest_value_in_auto_range
            .store((new_lowest * shift_multiplier).to_bits(), Ordering::Relaxed);

        let result = (|| {
            if self.integer_histogram.get_total_count() > mutation.count_at_index(0) {
                match mutation.shift_values_right_with_conversion_ratio(
                    number_of_binary_orders_of_magnitude,
                    new_integer_to_double_value_conversion_ratio,
                ) {
                    Ok(()) => {
                        new_lowest *= shift_multiplier;
                        new_highest *= shift_multiplier;
                    }
                    Err(_) => {
                        self.handle_shift_values_exception(&mut mutation, number_of_binary_orders_of_magnitude)?;
                        new_lowest /= shift_multiplier;
                    }
                }
            }
            new_lowest *= shift_multiplier;
            new_highest *= shift_multiplier;
            Ok(())
        })();

        if result.is_ok() {
            self.set_trackable_value_range_with_mutation(&mut mutation, new_lowest, new_highest);
            self.publish_range_generation();
        } else {
            self.set_trackable_value_range_fields(original_lowest, original_highest);
        }
        result
    }

    fn handle_shift_values_exception(
        &self,
        mutation: &mut ResizableStructuralMutation<'_>,
        number_of_binary_orders_of_magnitude: u32,
    ) -> Result<(), RecordError> {
        if !self.auto_resize.load(Ordering::Relaxed) {
            return Err(RecordError::ValueOutOfRangeResizeDisabled);
        }
        let highest_trackable_value = mutation.highest_trackable_value();
        let current_containing_order = find_containing_binary_order_of_magnitude_long(highest_trackable_value);
        let new_containing_order = current_containing_order + number_of_binary_orders_of_magnitude;
        if new_containing_order > 63 {
            return Err(RecordError::ValueOutOfRangeResizeDisabled);
        }
        let new_highest_trackable_value = (1_u64 << new_containing_order) - 1;
        mutation.resize(new_highest_trackable_value).map_err(RecordError::ResizeFailed)?;
        let configured_ratio = self.configured_highest_to_lowest_value_ratio.load(Ordering::Relaxed);
        self.configured_highest_to_lowest_value_ratio
            .store(configured_ratio << number_of_binary_orders_of_magnitude, Ordering::Relaxed);
        Ok(())
    }

    fn add_view_while_correcting_for_coordinated_omission<H: ReadableHistogram>(
        &self,
        other_view: H,
        other_ratio: f64,
        expected_interval_between_value_samples: f64,
    ) -> Result<(), RecordError> {
        let mut iterator = RecordedValuesIterator::from_readable(other_view);
        while let Some(value) = iterator.try_next().map_err(|_| RecordError::ConcurrentModification)? {
            let double_value = value.value_iterated_to as f64 * other_ratio;
            self.record_value_with_count_and_expected_interval(
                double_value,
                value.count_at_value_iterated_to,
                expected_interval_between_value_samples,
            )?;
        }
        Ok(())
    }

    pub(crate) fn percentiles_snapshot(
        &self,
        percentile_ticks_per_half_distance: u32,
    ) -> DoublePercentileIterator<ConcurrentDoubleReadView<'_>> {
        DoublePercentileIterator::from_readable(self.read_view(), percentile_ticks_per_half_distance)
    }

    pub(crate) fn linear_bucket_values_snapshot(&self, value_units_per_bucket: f64) -> DoubleLinearIterator<ConcurrentDoubleReadView<'_>> {
        DoubleLinearIterator::from_readable(self.read_view(), value_units_per_bucket)
    }

    pub(crate) fn logarithmic_bucket_values_snapshot(
        &self,
        value_units_in_first_bucket: f64,
        log_base: f64,
    ) -> DoubleLogarithmicIterator<ConcurrentDoubleReadView<'_>> {
        DoubleLogarithmicIterator::from_readable(self.read_view(), value_units_in_first_bucket, log_base)
    }

    pub(crate) fn all_values_snapshot(&self) -> DoubleAllValuesIterator<ConcurrentDoubleReadView<'_>> {
        DoubleAllValuesIterator::from_readable(self.read_view())
    }

    pub(crate) fn recorded_values_snapshot(&self) -> DoubleRecordedValuesIterator<ConcurrentDoubleReadView<'_>> {
        DoubleRecordedValuesIterator::from_readable(self.read_view())
    }
}

fn to_integer_value_clamped_with_ratio(value: f64, ratio: f64) -> u64 {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    let scaled = value * ratio;
    if scaled > u64::MAX as f64 {
        return u64::MAX;
    }
    scaled as u64
}

fn to_integer_value_clamped_for_histogram<H: ReadableHistogram>(histogram: &H, value: f64) -> u64 {
    to_integer_value_clamped_with_ratio(value, 1.0 / histogram.integer_to_double_value_conversion_ratio())
}

fn lowest_equivalent_value_for_histogram<H: ReadableHistogram>(histogram: &H, value: f64) -> f64 {
    let integer_value = to_integer_value_clamped_for_histogram(histogram, value);
    histogram.settings().lowest_equivalent_value(integer_value) as f64 * histogram.integer_to_double_value_conversion_ratio()
}

fn next_non_equivalent_value_for_histogram<H: ReadableHistogram>(histogram: &H, value: f64) -> f64 {
    let integer_value = to_integer_value_clamped_for_histogram(histogram, value);
    histogram.settings().next_non_equivalent_value(integer_value) as f64 * histogram.integer_to_double_value_conversion_ratio()
}

fn highest_equivalent_value_for_histogram<H: ReadableHistogram>(histogram: &H, value: f64) -> f64 {
    let next_non_equivalent_value = next_non_equivalent_value_for_histogram(histogram, value);
    let mut highest_equivalent_value = next_non_equivalent_value - (2.0 * ulp(next_non_equivalent_value));
    while highest_equivalent_value + ulp(highest_equivalent_value) < next_non_equivalent_value {
        highest_equivalent_value += ulp(highest_equivalent_value);
    }
    highest_equivalent_value
}

fn get_value_at_percentile_for_histogram<H: ReadableHistogram>(histogram: &H, percentile: f64) -> u64 {
    let one_below = util::next_below(percentile);
    let requested_percentile = one_below.clamp(0.0, 100.0);

    let total_count = histogram.get_total_count();
    let fractional_count = (requested_percentile / 100.0) * total_count as f64;
    let mut count_at_percentile = fractional_count.ceil() as u64;
    count_at_percentile = std::cmp::max(count_at_percentile, 1);

    let mut total_to_current_index: u64 = 0;
    for i in 0..histogram.array_length() {
        total_to_current_index += histogram.unsafe_get_count_at_index(i);
        if total_to_current_index >= count_at_percentile {
            let value_at_index = histogram.settings().value_from_index(i);
            return if percentile == 0.0 {
                histogram.settings().lowest_equivalent_value(value_at_index)
            } else {
                histogram.settings().highest_equivalent_value(value_at_index)
            };
        }
    }

    0
}

fn get_percentile_at_or_below_value_for_histogram<H: ReadableHistogram>(histogram: &H, value: u64) -> f64 {
    if histogram.get_total_count() == 0 {
        return 100.0;
    }
    let idx = histogram.settings().counts_array_index(value);
    let max_idx = histogram.array_length() - 1;
    let target_index = if idx > max_idx { max_idx } else { idx };
    let total_to_current_index = (0..=target_index).fold(0_f64, |t, i| t + histogram.unsafe_get_count_at_index(i) as f64);
    (100.0 * total_to_current_index) / histogram.get_total_count() as f64
}
