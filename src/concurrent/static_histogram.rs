use crate::concurrent::{concurrent_util, Snapshot};
use crate::concurrent::inline_backing_array::InlineBackingArray;
use crate::concurrent::recordable_histogram::RecordableHistogram;
use crate::core::*;
use crate::core::constants::*;
use crate::iteration::RecordedValuesIterator;
use std::convert::TryFrom;
use std::{mem, ptr};
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicPtr, AtomicU64};
use std::sync::atomic::Ordering;

#[repr(C)]
pub struct StaticHistogram<const N: usize> {
    meta_data: HistogramMetaData,
    settings: UnsafeCell<HistogramSettings>,
    raw_max_value: AtomicU64,
    raw_min_non_zero_value: AtomicU64,
    total_count: AtomicU64,
    pub(in crate::concurrent) counts: AtomicPtr<InlineBackingArray<AtomicU64>>,
}

impl<const N: usize> StaticHistogram<N> {
    pub fn new(highest_trackable_value: u64, significant_value_digits: u8) -> Result<StaticHistogram<N>, CreationError> {
        Self::with_low_high_sigvdig(1, highest_trackable_value, significant_value_digits)
    }
    pub fn with_low_high_sigvdig(
        lowest_discernible_value: u64,
        highest_trackable_value: u64,
        significant_value_digits: u8,
    ) -> Result<StaticHistogram<N>, CreationError> {
        let counts_array_length = u32::try_from(N)
            .map_err(|_| CreationError::RequiresExcessiveArrayLen)?;
        let settings = HistogramSettings::new(
            lowest_discernible_value,
            highest_trackable_value,
            significant_value_digits,
        )?;
        if settings.counts_array_length != counts_array_length {
            return Err(CreationError::CountsArrayLengthMismatch {
                expected: counts_array_length,
                actual: settings.counts_array_length,
            });
        }
        unsafe {
            let array_ptr = InlineBackingArray::new(counts_array_length);
            Ok(StaticHistogram::<N> {
                meta_data: HistogramMetaData::new(),
                settings: UnsafeCell::new(settings),
                raw_max_value: AtomicU64::new(ORIGINAL_MAX),
                raw_min_non_zero_value: AtomicU64::new(ORIGINAL_MIN),
                total_count: AtomicU64::new(0),
                counts: AtomicPtr::new(array_ptr),
            })
        }
    }

    #[inline(always)]
    pub fn counts_array_length(&self) -> u32 {
        unsafe { (*self.settings.get()).counts_array_length }
    }

    pub(crate) fn normalizing_index_offset(&self) -> i32 {
        unsafe { (*self.counts.load(Ordering::Relaxed)).normalizing_index_offset() }
    }

    pub(crate) fn set_normalizing_index_offset(&self, offset: i32) {
        unsafe { (*self.counts.load(Ordering::Relaxed)).set_normalizing_index_offset(offset) };
    }

    pub(crate) fn set_integer_to_double_value_conversion_ratio(&self, ratio: f64) {
        unsafe {
            let settings = &mut *self.settings.get();
            settings.integer_to_double_value_conversion_ratio = ratio;
            settings.double_to_integer_value_conversion_ratio = 1.0 / ratio;
        }
    }

    pub(crate) fn set_count_at_index(&self, index: u32, count: u64) {
        unsafe {
            let counts = &*self.counts.load(Ordering::Relaxed);
            let normalized_index = util::normalize_index(
                index,
                counts.normalizing_index_offset(),
                counts.length(),
            );
            counts
                .get_unchecked(normalized_index)
                .store(count, Ordering::Relaxed);
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
            let idx = (*self.settings.get()).counts_array_index(value);

            let counts = &*self.counts.load(Ordering::Relaxed);
            if idx < counts.length() {
                let normalized_index = util::normalize_index(
                    idx,
                    counts.normalizing_index_offset(),
                    counts.length(),
                );
                let c = counts.get_unchecked(normalized_index);
                c.fetch_add(count, Ordering::Relaxed);
                self.update_min_and_max(value);
                self.total_count.fetch_add(count, Ordering::Relaxed);
                Ok(())
            } else {
                let last_idx = counts.length() - 1;
                let normalized_index = util::normalize_index(
                    last_idx,
                    counts.normalizing_index_offset(),
                    counts.length(),
                );
                let c = counts.get_unchecked(normalized_index);
                c.fetch_add(count, Ordering::Relaxed);
                self.update_min_and_max(value);
                self.total_count.fetch_add(count, Ordering::Relaxed);
                Ok(())
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
        concurrent_util::update_max_value(&self.settings, &self.raw_max_value, value);
    }

    #[inline]
    fn update_min_non_zero_value(&self, value: u64) {
        concurrent_util::update_min_non_zero_value(&self.settings, &self.raw_min_non_zero_value, value);
    }

    pub(crate) fn shift_values_left(&self, number_of_binary_orders_of_magnitude: u32) -> Result<(), ShiftError> {
        if number_of_binary_orders_of_magnitude == 0 {
            return Ok(());
        }
        if self.get_total_count() == self.unsafe_get_count_at_index(0) {
            return Ok(());
        }

        let shift_amount = number_of_binary_orders_of_magnitude << self.settings().sub_bucket_half_count_magnitude;
        let max_value_index = self.settings().counts_array_index(self.get_max_value());
        if max_value_index >= (self.counts_array_length() - shift_amount) {
            return Err(ShiftError::Overflow);
        }

        let max_before = self.raw_max_value.swap(ORIGINAL_MAX, Ordering::Relaxed);
        let min_before = self.raw_min_non_zero_value.swap(ORIGINAL_MIN, Ordering::Relaxed);
        let lowest_half_bucket_populated =
            min_before < ((self.settings().sub_bucket_half_count as u64) << self.settings().unit_magnitude);

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

        let shift_amount = self.settings().sub_bucket_half_count * number_of_binary_orders_of_magnitude;
        let min_non_zero_value_index = self.settings().counts_array_index(self.get_min_non_zero_value());
        if min_non_zero_value_index < shift_amount + self.settings().sub_bucket_half_count {
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

    fn shift_normalizing_index_by_offset(
        &self,
        offset_to_add: i32,
        lowest_half_bucket_populated: bool,
    ) -> Result<(), ShiftError> {
        unsafe {
            let counts = &*self.counts.load(Ordering::Relaxed);
            let pre_shift_zero_index = util::normalize_index(
                0,
                counts.normalizing_index_offset(),
                counts.length(),
            );
            let zero_value_count = counts
                .get_unchecked(pre_shift_zero_index)
                .load(Ordering::Relaxed);
            counts
                .get_unchecked(pre_shift_zero_index)
                .store(0, Ordering::Relaxed);

            counts.set_normalizing_index_offset(counts.normalizing_index_offset() + offset_to_add);

            if lowest_half_bucket_populated {
                if offset_to_add <= 0 {
                    return Err(ShiftError::Underflow);
                }
                self.shift_lowest_half_bucket_contents_left(counts, offset_to_add as u32, pre_shift_zero_index);
            }

            let new_zero_index = util::normalize_index(
                0,
                counts.normalizing_index_offset(),
                counts.length(),
            );
            counts
                .get_unchecked(new_zero_index)
                .store(zero_value_count, Ordering::Relaxed);
        }
        Ok(())
    }

    fn shift_lowest_half_bucket_contents_left(
        &self,
        counts: &InlineBackingArray<AtomicU64>,
        shift_amount: u32,
        pre_shift_zero_index: u32,
    ) {
        let number_of_binary_orders_of_magnitude =
            shift_amount >> self.settings().sub_bucket_half_count_magnitude;
        for from_index in 1..self.settings().sub_bucket_half_count {
            let to_value = self.settings().value_from_index(from_index) << number_of_binary_orders_of_magnitude;
            let to_index = self.settings().counts_array_index(to_value);
            let normalized_to_index = util::normalize_index(
                to_index,
                counts.normalizing_index_offset(),
                counts.length(),
            );
            let from_normalized_index = from_index + pre_shift_zero_index;
            let count_at_from_index = unsafe {
                counts
                    .get_unchecked(from_normalized_index)
                    .load(Ordering::Relaxed)
            };
            unsafe {
                counts
                    .get_unchecked(normalized_to_index)
                    .store(count_at_from_index, Ordering::Relaxed);
                counts
                    .get_unchecked(from_normalized_index)
                    .store(0, Ordering::Relaxed);
            }
        }
    }

    #[inline(always)]
    pub fn is_auto_resize(&self) -> bool {
        unsafe { (*self.settings.get()).auto_resize }
    }

    pub fn get_count_at_index(&self, index: u32) -> Option<u64> {
        unsafe {
            let counts = &*self.counts.load(Ordering::Relaxed);
            if index >= counts.length() {
                None
            } else {
                let normalized_index = util::normalize_index(
                    index,
                    counts.normalizing_index_offset(),
                    counts.length(),
                );
                Some(counts.get_unchecked(normalized_index).load(Ordering::Relaxed))
            }
        }
    }

    pub(crate) fn unsafe_get_count_at_index(&self, index: u32) -> u64 {
        unsafe {
            let counts = &*self.counts.load(Ordering::Relaxed);

            let normalized_index = util::normalize_index(
                index,
                counts.normalizing_index_offset(),
                counts.length(),
            );
            counts.get_unchecked(normalized_index).load(Ordering::Relaxed)
        }
    }

    pub fn get_total_count(&self) -> u64 {
        self.total_count.load(Ordering::Relaxed)
    }
    pub fn get_max_value(&self) -> u64 {
        self.settings()
            .get_max_value(self.raw_max_value.load(Ordering::Relaxed))
    }
    pub fn get_min_non_zero_value(&self) -> u64 {
        self.settings()
            .get_min_non_zero_value(self.raw_min_non_zero_value.load(Ordering::Relaxed))
    }

    pub unsafe fn clear_counts(&self) {
        let counts = self.counts.load(Ordering::Relaxed);
        let settings = &*self.settings.get();
        for i in 0..self.counts_array_length() {
            (*counts).get_unchecked(i).store(0, Ordering::Relaxed);
        }
        self.total_count.store(0, Ordering::Relaxed);
        self.raw_max_value
            .store(ORIGINAL_MAX | settings.unit_magnitude_mask, Ordering::Relaxed);
        self.raw_min_non_zero_value
            .store(ORIGINAL_MIN, Ordering::Relaxed);
        let meta_data = &self.meta_data as *const HistogramMetaData as *mut HistogramMetaData;
        (*meta_data).clear();
    }

    unsafe fn copy_counts(&self, source: &InlineBackingArray<AtomicU64>, target: &mut InlineBackingArray<AtomicU64>) {
        ptr::copy_nonoverlapping(
            source.get_array_ptr(),
            (*target).get_array_ptr(),
            self.counts_array_length() as usize,
        );
        target.set_normalizing_index_offset(source.normalizing_index_offset());
    }

    fn equals(&self, other: &Self) -> bool {
        if ptr::eq(self, other) {
            return true;
        }
        if !self.settings().equals(other.settings()) {
            return false;
        }
        check_eq!(self.get_total_count(), other.get_total_count());
        check_eq!(self.get_max_value(), other.get_max_value());
        check_eq!(
            self.get_min_non_zero_value(),
            other.get_min_non_zero_value()
        );
        let self_len = self.counts_array_length();
        let other_len = other.counts_array_length();
        if self_len == other_len {
            for i in 0..self_len {
                check_eq!(
                    self.unsafe_get_count_at_index(i),
                    other.unsafe_get_count_at_index(i)
                )
            }
        } else {
            let other_last = other_len - 1;
            for value in RecordedValuesIterator::new(self) {
                let mut other_index = other.settings().counts_array_index(value.value_iterated_to);
                if other_index > other_last {
                    other_index = other_last;
                }
                let other_count = other.unsafe_get_count_at_index(other_index);
                check_eq!(value.count_at_value_iterated_to, other_count);
            }
        }
        return true;
    }

    // this is really, really unsafe. use only if you can guarantee unique access
    pub unsafe fn unsafe_as_snapshot(&self) -> Snapshot<'_, Self> {
        #[allow(mutable_transmutes)]
        Snapshot::new(mem::transmute(self))
    }

    pub fn as_snapshot(&mut self) -> Snapshot<'_, Self> {
        unsafe { Snapshot::new(self) }
    }
}

impl<const N: usize> ConstructableHistogram for StaticHistogram<N> {
    fn new(lowest_discernible_value: u64, highest_trackable_value: u64, significant_value_digits: u8) -> Result<Self, CreationError> {
        StaticHistogram::<N>::with_low_high_sigvdig(
            lowest_discernible_value,
            highest_trackable_value,
            significant_value_digits,
        )
    }

    fn establish_internal_tracking_values(&mut self) {
        self.raw_max_value.store(
            ORIGINAL_MAX | self.settings().unit_magnitude_mask,
            Ordering::Relaxed,
        );
        self.raw_min_non_zero_value.store(
            ORIGINAL_MIN,
            Ordering::Relaxed,
        );
        let array_length = self.counts_array_length();
        let (new_max, new_min, new_total) = util::recalculate_internal_tracking_values(self, array_length);
        new_max.map(|mi| {
            let new_max = self.settings()
                .highest_equivalent_value(self.settings().value_from_index(mi));
            self.update_max_value(new_max);
        });
        new_min.map(|mi| {
            let new_min = self.settings().value_from_index(mi);
            self.update_min_non_zero_value(new_min);
        });
        self.total_count.store(new_total, Ordering::Relaxed);
    }
}

impl<const N: usize> RecordableHistogram for StaticHistogram<N> {
    fn fresh(settings: &HistogramSettings) -> Result<StaticHistogram<N>, CreationError> {
        let lowest_discernable = settings.lowest_discernible_value;
        let highest_trackable = settings.highest_trackable_value;
        let sigvdig = settings.number_of_significant_value_digits as u8;
        StaticHistogram::<N>::with_low_high_sigvdig(lowest_discernable, highest_trackable, sigvdig)
    }
    #[inline(always)]
    fn meta_data_mut(&mut self) -> &mut HistogramMetaData {
        &mut self.meta_data
    }
    #[inline(always)]
    unsafe fn clear_counts(&self) {
        StaticHistogram::<N>::clear_counts(self);
    }
    fn equals(&mut self, other: &mut Self) -> bool {
        StaticHistogram::<N>::equals(self, other)
    }
    #[inline(always)]
    fn record_value(&self, value: u64) -> Result<(), RecordError> {
        StaticHistogram::<N>::record_value(self, value)
    }
    #[inline(always)]
    fn record_value_with_count(&self, value: u64, count: u64) -> Result<(), RecordError> {
        StaticHistogram::<N>::record_value_with_count(self, value, count)
    }
}

impl<const N: usize> ReadableHistogram for StaticHistogram<N> {
    #[inline(always)]
    fn settings(&self) -> &HistogramSettings {
        unsafe { &*self.settings.get() }
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
        StaticHistogram::<N>::get_max_value(self)
    }
    fn meta_data(&self) -> &HistogramMetaData { &self.meta_data }
}
