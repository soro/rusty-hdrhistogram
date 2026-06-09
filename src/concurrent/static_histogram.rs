use crate::concurrent::concurrent_util;
use crate::concurrent::inline_backing_array::InlineBackingArray;
use crate::concurrent::recordable_histogram::RecordableHistogram;
use crate::concurrent::snapshot::{FixedSnapshot, Snapshot};
use crate::core::constants::*;
use crate::core::*;
use crate::iteration::RecordedValuesIterator;
use std::ptr;
use std::sync::atomic::Ordering;
use std::sync::atomic::{AtomicPtr, AtomicU64};

#[repr(C)]
pub struct FixedConcurrentHistogram {
    meta_data: HistogramMetaData,
    layout: HistogramLayout,
    raw_max_value: AtomicU64,
    raw_min_non_zero_value: AtomicU64,
    total_count: AtomicU64,
    pub(in crate::concurrent) counts: AtomicPtr<InlineBackingArray<AtomicU64>>,
}

const DEFAULT_SIGNIFICANT_VALUE_DIGITS: u8 = 3;

/// Builder for a fixed-range concurrent integer histogram.
///
/// Builders default to three decimal significant digits. Use
/// [`significant_digits`](Self::significant_digits) to choose a different
/// precision.
pub struct FixedConcurrentHistogramBuilder {
    lowest_discernible_value: u64,
    highest_trackable_value: u64,
    significant_value_digits: u8,
}

impl FixedConcurrentHistogramBuilder {
    pub fn new() -> Self {
        FixedConcurrentHistogramBuilder {
            lowest_discernible_value: 1,
            highest_trackable_value: 2,
            significant_value_digits: DEFAULT_SIGNIFICANT_VALUE_DIGITS,
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

    pub fn build(self) -> Result<FixedConcurrentHistogram, CreationError> {
        FixedConcurrentHistogram::with_low_high_sigvdig(
            self.lowest_discernible_value,
            self.highest_trackable_value,
            self.significant_value_digits,
        )
    }
}

impl Drop for FixedConcurrentHistogram {
    fn drop(&mut self) {
        let counts = *self.counts.get_mut();
        debug_assert!(!counts.is_null());
        if !counts.is_null() {
            unsafe { (*counts).dealloc() };
        }
    }
}

impl FixedConcurrentHistogram {
    pub fn builder() -> FixedConcurrentHistogramBuilder {
        FixedConcurrentHistogramBuilder::new()
    }

    pub(crate) fn new(highest_trackable_value: u64, significant_value_digits: u8) -> Result<FixedConcurrentHistogram, CreationError> {
        Self::with_low_high_sigvdig(1, highest_trackable_value, significant_value_digits)
    }
    pub(crate) fn with_low_high_sigvdig(
        lowest_discernible_value: u64,
        highest_trackable_value: u64,
        significant_value_digits: u8,
    ) -> Result<FixedConcurrentHistogram, CreationError> {
        let layout = HistogramLayout::new(lowest_discernible_value, highest_trackable_value, significant_value_digits)?;
        let metadata = layout.initial_metadata_for_highest(highest_trackable_value)?;
        unsafe {
            let array_ptr = InlineBackingArray::new(metadata);
            Ok(FixedConcurrentHistogram {
                meta_data: HistogramMetaData::new(),
                layout,
                raw_max_value: AtomicU64::new(ORIGINAL_MAX),
                raw_min_non_zero_value: AtomicU64::new(ORIGINAL_MIN),
                total_count: AtomicU64::new(0),
                counts: AtomicPtr::new(array_ptr),
            })
        }
    }

    #[inline(always)]
    pub fn counts_array_length(&self) -> u32 {
        unsafe { (*self.counts.load(Ordering::Relaxed)).length() }
    }

    pub fn settings(&self) -> HistogramSettings {
        let metadata = unsafe { (*self.counts.load(Ordering::Relaxed)).metadata() };
        self.layout.settings_snapshot(metadata, false)
    }

    pub(crate) fn normalizing_index_offset(&self) -> i32 {
        unsafe { (*self.counts.load(Ordering::Relaxed)).normalizing_index_offset() }
    }

    pub(crate) fn set_normalizing_index_offset(&self, offset: i32) {
        unsafe { (*self.counts.load(Ordering::Relaxed)).set_normalizing_index_offset(offset) };
    }

    pub(crate) fn integer_to_double_value_conversion_ratio(&self) -> f64 {
        unsafe { (*self.counts.load(Ordering::Relaxed)).integer_to_double_value_conversion_ratio() }
    }

    pub(crate) fn double_to_integer_value_conversion_ratio(&self) -> f64 {
        unsafe { (*self.counts.load(Ordering::Relaxed)).double_to_integer_value_conversion_ratio() }
    }

    pub(crate) fn set_integer_to_double_value_conversion_ratio(&self, ratio: f64) {
        unsafe { (*self.counts.load(Ordering::Relaxed)).set_integer_to_double_value_conversion_ratio(ratio) };
    }

    pub(crate) fn set_count_at_index(&self, index: u32, count: u64) {
        unsafe {
            let counts = &*self.counts.load(Ordering::Relaxed);
            let normalized_index = util::normalize_index(index, counts.normalizing_index_offset(), counts.length());
            counts.get_unchecked(normalized_index).store(count, Ordering::Relaxed);
        }
    }

    #[inline(always)]
    pub fn record_value(&self, value: u64) -> Result<(), RecordError> {
        self.record_value_with_count(value, 1)
    }

    #[inline(always)]
    pub fn record_value_with_count(&self, value: u64, count: u64) -> Result<(), RecordError> {
        self.record_count_at_value(count, value)
    }

    #[inline(always)]
    fn record_count_at_value(&self, count: u64, value: u64) -> Result<(), RecordError> {
        unsafe {
            let idx = self.layout.counts_array_index(value);

            let counts = &*self.counts.load(Ordering::Relaxed);
            if idx < counts.length() {
                let normalized_index = util::normalize_index(idx, counts.normalizing_index_offset(), counts.length());
                let c = counts.get_unchecked(normalized_index);
                c.fetch_add(count, Ordering::Relaxed);
                self.update_min_and_max(value);
                self.total_count.fetch_add(count, Ordering::Relaxed);
                Ok(())
            } else {
                Err(RecordError::ValueOutOfRangeResizeDisabled)
            }
        }
    }

    #[inline(always)]
    fn update_min_and_max(&self, value: u64) {
        if value > self.raw_max_value.load(Ordering::Relaxed) {
            self.update_max_value(value)
        }
        if value < self.raw_min_non_zero_value.load(Ordering::Relaxed) {
            self.update_min_non_zero_value(value)
        }
    }

    #[inline]
    fn update_max_value(&self, value: u64) {
        concurrent_util::update_max_value(&self.layout, &self.raw_max_value, value);
    }

    #[inline]
    fn update_min_non_zero_value(&self, value: u64) {
        concurrent_util::update_min_non_zero_value(&self.layout, &self.raw_min_non_zero_value, value);
    }

    pub(crate) fn shift_values_left(&self, number_of_binary_orders_of_magnitude: u32) -> Result<(), ShiftError> {
        if number_of_binary_orders_of_magnitude == 0 {
            return Ok(());
        }
        if self.get_total_count() == self.unsafe_get_count_at_index(0) {
            return Ok(());
        }

        let shift_amount = number_of_binary_orders_of_magnitude << self.layout.sub_bucket_half_count_magnitude;
        let max_value_index = self.layout.counts_array_index(self.get_max_value());
        if max_value_index >= (self.counts_array_length() - shift_amount) {
            return Err(ShiftError::Overflow);
        }

        let max_before = self.raw_max_value.swap(ORIGINAL_MAX, Ordering::Relaxed);
        let min_before = self.raw_min_non_zero_value.swap(ORIGINAL_MIN, Ordering::Relaxed);
        let lowest_half_bucket_populated = min_before < ((self.layout.sub_bucket_half_count as u64) << self.layout.unit_magnitude);

        self.shift_normalizing_index_by_offset(shift_amount as i32, lowest_half_bucket_populated)?;

        self.update_min_and_max(max_before << number_of_binary_orders_of_magnitude);
        if min_before != ORIGINAL_MIN {
            self.update_min_and_max(min_before << number_of_binary_orders_of_magnitude);
        }
        Ok(())
    }

    pub(crate) fn shift_values_right(&self, number_of_binary_orders_of_magnitude: u32) -> Result<(), ShiftError> {
        if number_of_binary_orders_of_magnitude == 0 {
            return Ok(());
        }
        if self.get_total_count() == self.unsafe_get_count_at_index(0) {
            return Ok(());
        }

        let shift_amount = self.layout.sub_bucket_half_count * number_of_binary_orders_of_magnitude;
        let min_non_zero_value_index = self.layout.counts_array_index(self.get_min_non_zero_value());
        if min_non_zero_value_index < shift_amount + self.layout.sub_bucket_half_count {
            return Err(ShiftError::Underflow);
        }

        let max_before = self.raw_max_value.swap(ORIGINAL_MAX, Ordering::Relaxed);
        let min_before = self.raw_min_non_zero_value.swap(ORIGINAL_MIN, Ordering::Relaxed);

        self.shift_normalizing_index_by_offset(-(shift_amount as i32), false)?;

        self.update_min_and_max(max_before >> number_of_binary_orders_of_magnitude);
        if min_before != ORIGINAL_MIN {
            self.update_min_and_max(min_before >> number_of_binary_orders_of_magnitude);
        }
        Ok(())
    }

    fn shift_normalizing_index_by_offset(&self, offset_to_add: i32, lowest_half_bucket_populated: bool) -> Result<(), ShiftError> {
        unsafe {
            let counts = &*self.counts.load(Ordering::Relaxed);
            let pre_shift_zero_index = util::normalize_index(0, counts.normalizing_index_offset(), counts.length());
            let zero_value_count = counts.get_unchecked(pre_shift_zero_index).load(Ordering::Relaxed);
            counts.get_unchecked(pre_shift_zero_index).store(0, Ordering::Relaxed);

            counts.set_normalizing_index_offset(counts.normalizing_index_offset() + offset_to_add);

            if lowest_half_bucket_populated {
                if offset_to_add <= 0 {
                    return Err(ShiftError::Underflow);
                }
                self.shift_lowest_half_bucket_contents_left(counts, offset_to_add as u32, pre_shift_zero_index);
            }

            let new_zero_index = util::normalize_index(0, counts.normalizing_index_offset(), counts.length());
            counts.get_unchecked(new_zero_index).store(zero_value_count, Ordering::Relaxed);
        }
        Ok(())
    }

    fn shift_lowest_half_bucket_contents_left(&self, counts: &InlineBackingArray<AtomicU64>, shift_amount: u32, pre_shift_zero_index: u32) {
        let number_of_binary_orders_of_magnitude = shift_amount >> self.layout.sub_bucket_half_count_magnitude;
        for from_index in 1..self.layout.sub_bucket_half_count {
            let to_value = self.layout.value_from_index(from_index) << number_of_binary_orders_of_magnitude;
            let to_index = self.layout.counts_array_index(to_value);
            let normalized_to_index = util::normalize_index(to_index, counts.normalizing_index_offset(), counts.length());
            let from_normalized_index = from_index + pre_shift_zero_index;
            let count_at_from_index = unsafe { counts.get_unchecked(from_normalized_index).load(Ordering::Relaxed) };
            unsafe {
                counts
                    .get_unchecked(normalized_to_index)
                    .store(count_at_from_index, Ordering::Relaxed);
                counts.get_unchecked(from_normalized_index).store(0, Ordering::Relaxed);
            }
        }
    }

    #[inline(always)]
    pub fn is_auto_resize(&self) -> bool {
        false
    }

    pub fn get_count_at_index(&self, index: u32) -> Option<u64> {
        unsafe {
            let counts = &*self.counts.load(Ordering::Relaxed);
            if index >= counts.length() {
                None
            } else {
                let normalized_index = util::normalize_index(index, counts.normalizing_index_offset(), counts.length());
                Some(counts.get_unchecked(normalized_index).load(Ordering::Relaxed))
            }
        }
    }

    pub(crate) fn unsafe_get_count_at_index(&self, index: u32) -> u64 {
        unsafe {
            let counts = &*self.counts.load(Ordering::Relaxed);

            let normalized_index = util::normalize_index(index, counts.normalizing_index_offset(), counts.length());
            counts.get_unchecked(normalized_index).load(Ordering::Relaxed)
        }
    }

    pub fn get_total_count(&self) -> u64 {
        self.total_count.load(Ordering::Relaxed)
    }
    pub fn get_max_value(&self) -> u64 {
        self.layout.get_max_value(self.raw_max_value.load(Ordering::Relaxed))
    }
    pub fn get_min_non_zero_value(&self) -> u64 {
        self.layout
            .get_min_non_zero_value(self.raw_min_non_zero_value.load(Ordering::Relaxed))
    }

    pub(crate) fn clear_counts_for_reuse(&mut self) {
        let counts = *self.counts.get_mut();
        let counts_len = unsafe { (*counts).length() };
        for i in 0..counts_len {
            unsafe { (*counts).get_unchecked(i) }.store(0, Ordering::Relaxed);
        }
        unsafe { (*counts).set_normalizing_index_offset(0) };
        self.total_count.store(0, Ordering::Relaxed);
        self.raw_max_value
            .store(ORIGINAL_MAX | self.layout.unit_magnitude_mask, Ordering::Relaxed);
        self.raw_min_non_zero_value.store(ORIGINAL_MIN, Ordering::Relaxed);
        self.meta_data.clear();
    }

    unsafe fn copy_counts(&self, source: &InlineBackingArray<AtomicU64>, target: &mut InlineBackingArray<AtomicU64>) {
        ptr::copy_nonoverlapping(
            source.get_array_ptr(),
            (*target).get_array_ptr(),
            self.counts_array_length() as usize,
        );
        target.set_normalizing_index_offset(source.normalizing_index_offset());
        target.set_integer_to_double_value_conversion_ratio(source.integer_to_double_value_conversion_ratio());
    }

    fn equals(&self, other: &Self) -> bool {
        if ptr::eq(self, other) {
            return true;
        }
        if !self.settings().equals(&other.settings()) {
            return false;
        }
        check_eq!(
            self.integer_to_double_value_conversion_ratio(),
            other.integer_to_double_value_conversion_ratio()
        );
        check_eq!(self.get_total_count(), other.get_total_count());
        check_eq!(self.get_max_value(), other.get_max_value());
        check_eq!(self.get_min_non_zero_value(), other.get_min_non_zero_value());
        let self_len = self.counts_array_length();
        let other_len = other.counts_array_length();
        if self_len == other_len {
            for i in 0..self_len {
                check_eq!(self.unsafe_get_count_at_index(i), other.unsafe_get_count_at_index(i))
            }
        } else {
            let other_last = other_len - 1;
            let mut iterator = RecordedValuesIterator::from_readable(self);
            loop {
                let Some(value) = (match iterator.try_next() {
                    Ok(value) => value,
                    Err(_) => return false,
                }) else {
                    break;
                };
                let mut other_index = other.layout.counts_array_index(value.value_iterated_to);
                if other_index > other_last {
                    other_index = other_last;
                }
                let other_count = other.unsafe_get_count_at_index(other_index);
                check_eq!(value.count_at_value_iterated_to, other_count);
            }
        }
        true
    }

    // Use only when callers can guarantee no concurrent structural mutation.
    pub(crate) unsafe fn unsafe_as_snapshot(&self) -> Snapshot<'_, Self> {
        Snapshot::new(self)
    }

    pub fn as_snapshot(&mut self) -> FixedSnapshot<'_> {
        FixedSnapshot::new(Snapshot::new(self))
    }
}

impl ConstructableHistogram for FixedConcurrentHistogram {
    fn new(lowest_discernible_value: u64, highest_trackable_value: u64, significant_value_digits: u8) -> Result<Self, CreationError> {
        FixedConcurrentHistogram::with_low_high_sigvdig(lowest_discernible_value, highest_trackable_value, significant_value_digits)
    }

    fn establish_internal_tracking_values(&mut self) {
        self.raw_max_value
            .store(ORIGINAL_MAX | self.layout.unit_magnitude_mask, Ordering::Relaxed);
        self.raw_min_non_zero_value.store(ORIGINAL_MIN, Ordering::Relaxed);
        let array_length = self.counts_array_length();
        let (new_max, new_min, new_total) = util::recalculate_internal_tracking_values(self, array_length);
        if let Some(mi) = new_max {
            let new_max = self.layout.highest_equivalent_value(self.layout.value_from_index(mi));
            self.update_max_value(new_max);
        }
        if let Some(mi) = new_min {
            let new_min = self.layout.value_from_index(mi);
            self.update_min_non_zero_value(new_min);
        }
        self.total_count.store(new_total, Ordering::Relaxed);
    }
}

impl RecordableHistogram for FixedConcurrentHistogram {
    fn fresh(settings: &HistogramSettings) -> Result<FixedConcurrentHistogram, CreationError> {
        FixedConcurrentHistogram::builder()
            .lowest_discernible_value(settings.lowest_discernible_value)
            .highest_trackable_value(settings.highest_trackable_value)
            .significant_digits(settings.number_of_significant_value_digits as u8)
            .build()
    }
    #[inline(always)]
    fn meta_data_mut(&mut self) -> &mut HistogramMetaData {
        &mut self.meta_data
    }
    #[inline(always)]
    fn clear_counts_for_reuse(&mut self) {
        FixedConcurrentHistogram::clear_counts_for_reuse(self);
    }
    fn equals(&self, other: &Self) -> bool {
        FixedConcurrentHistogram::equals(self, other)
    }
    fn get_min_non_zero_value(&self) -> u64 {
        FixedConcurrentHistogram::get_min_non_zero_value(self)
    }
    #[inline(always)]
    fn record_value(&self, value: u64) -> Result<(), RecordError> {
        FixedConcurrentHistogram::record_value(self, value)
    }
    #[inline(always)]
    fn record_value_with_count(&self, value: u64, count: u64) -> Result<(), RecordError> {
        FixedConcurrentHistogram::record_value_with_count(self, value, count)
    }
}

impl crate::core::readable_histogram::sealed::Sealed for FixedConcurrentHistogram {}

impl ReadableHistogram for FixedConcurrentHistogram {
    #[inline(always)]
    fn settings(&self) -> HistogramSettings {
        self.settings()
    }
    fn array_length(&self) -> u32 {
        self.counts_array_length()
    }
    fn get_total_count(&self) -> u64 {
        self.get_total_count()
    }
    fn unsafe_get_count_at_index(&self, idx: u32) -> u64 {
        self.unsafe_get_count_at_index(idx)
    }
    fn get_max_value(&self) -> u64 {
        FixedConcurrentHistogram::get_max_value(self)
    }
    fn meta_data(&self) -> &HistogramMetaData {
        &self.meta_data
    }
    fn integer_to_double_value_conversion_ratio(&self) -> f64 {
        FixedConcurrentHistogram::integer_to_double_value_conversion_ratio(self)
    }

    fn normalizing_index_offset(&self) -> i32 {
        FixedConcurrentHistogram::normalizing_index_offset(self)
    }
}
