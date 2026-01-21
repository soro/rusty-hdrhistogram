use crate::concurrent::{concurrent_util, Snapshot, WriterReaderPhaser};
use crate::concurrent::inline_backing_array::InlineBackingArray;
use crate::concurrent::recordable_histogram::RecordableHistogram;
use crate::core::*;
use crate::core::constants::*;
use crate::iteration::RecordedValuesIterator;
use std::{mem, ptr};
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicPtr, AtomicU64};
use std::sync::atomic::Ordering;

#[repr(C)]
pub struct ResizableHistogram {
    meta_data: HistogramMetaData,
    settings: UnsafeCell<HistogramSettings>,
    wrp: WriterReaderPhaser,
    raw_max_value: AtomicU64,
    raw_min_non_zero_value: AtomicU64,
    total_count: AtomicU64,
    pub(in concurrent) active_counts: AtomicPtr<InlineBackingArray<AtomicU64>>,
    inactive_counts: AtomicPtr<InlineBackingArray<AtomicU64>>,
}

impl Drop for ResizableHistogram {
    fn drop(&mut self) {
        unsafe {
            self.wrp.reader_lock().flip();
            (*self.active_counts.load(Ordering::SeqCst)).dealloc();
            (*self.inactive_counts.load(Ordering::SeqCst)).dealloc();
        }
    }
}

unsafe impl Send for ResizableHistogram {}
unsafe impl Sync for ResizableHistogram {}

impl ResizableHistogram {
    pub fn new(significant_value_digits: u8) -> Result<ResizableHistogram, CreationError> {
        ResizableHistogram::with_sigvdig(significant_value_digits)
    }
    pub fn with_sigvdig(significant_value_digits: u8) -> Result<ResizableHistogram, CreationError> {
        ResizableHistogram::with_high_sigvdig(2, significant_value_digits)
    }
    pub fn with_high_sigvdig(highest_trackable_value: u64, significant_value_digits: u8) -> Result<ResizableHistogram, CreationError> {
        ResizableHistogram::with_low_high_sigvdig(1, highest_trackable_value, significant_value_digits)
    }
    pub fn with_low_high_sigvdig(
        lowest_discernible_value: u64,
        highest_trackable_value: u64,
        significant_value_digits: u8,
    ) -> Result<ResizableHistogram, CreationError> {
        let mut settings = HistogramSettings::new(
            lowest_discernible_value,
            highest_trackable_value,
            significant_value_digits,
        )?;
        settings.auto_resize = true;
        unsafe {
            let active_array_ptr = InlineBackingArray::new(settings.counts_array_length);
            let inactive_array_ptr = InlineBackingArray::new(settings.counts_array_length);
            Ok(ResizableHistogram {
                meta_data: HistogramMetaData::new(),
                wrp: WriterReaderPhaser::new(),
                settings: UnsafeCell::new(settings),
                raw_max_value: AtomicU64::new(ORIGINAL_MAX),
                raw_min_non_zero_value: AtomicU64::new(ORIGINAL_MIN),
                total_count: AtomicU64::new(0),
                active_counts: AtomicPtr::new(active_array_ptr),
                inactive_counts: AtomicPtr::new(inactive_array_ptr),
            })
        }
    }

    pub fn counts_array_length(&self) -> u32 {
        unsafe { (*self.settings.get()).counts_array_length }
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
    pub(crate) fn record_value_with_count_strict(&self, value: u64, count: u64) -> Result<(), RecordError> {
        unsafe {
            let settings = &*self.settings.get();
            let idx = settings.counts_array_index(value);
            if idx >= settings.counts_array_length {
                return Err(RecordError::ValueOutOfRangeResizeDisabled);
            }
            {
                let _csg = self.wrp.begin_writer_critical_section();
                let active_counts = &*self.active_counts.load(Ordering::Relaxed);
                let normalized_index = util::normalize_index(
                    idx,
                    active_counts.normalizing_index_offset(),
                    active_counts.length(),
                );
                let c = active_counts.get_unchecked(normalized_index);
                c.fetch_add(count, Ordering::Relaxed);
            }
            self.update_min_and_max(value);
            self.total_count.fetch_add(count, Ordering::Relaxed);
            Ok(())
        }
    }

    #[inline(always)]
    fn record_count_at_value(&self, count: u64, value: u64) -> Result<(), RecordError> {
        unsafe {
            let idx = (*self.settings.get()).counts_array_index(value);

            if idx < (*self.settings.get()).counts_array_length {
                {
                    let _csg = self.wrp.begin_writer_critical_section();
                    let active_counts = &*self.active_counts.load(Ordering::Relaxed);
                    let normalized_index = util::normalize_index(
                        idx,
                        active_counts.normalizing_index_offset(),
                        active_counts.length(),
                    );
                    let c = active_counts.get_unchecked(normalized_index);
                    c.fetch_add(count, Ordering::Relaxed);
                }
                self.update_min_and_max(value);
                self.total_count.fetch_add(count, Ordering::Relaxed);
                Ok(())
            } else if !self.is_auto_resize() {
                {
                    let _csg = self.wrp.begin_writer_critical_section();
                    let active_counts = &*self.active_counts.load(Ordering::Relaxed);
                    let last_idx = (*self.settings.get()).counts_array_length - 1;
                    let normalized_index = util::normalize_index(
                        last_idx,
                        active_counts.normalizing_index_offset(),
                        active_counts.length(),
                    );
                    let c = active_counts.get_unchecked(normalized_index);
                    c.fetch_add(count, Ordering::Relaxed);
                }
                self.update_min_and_max(value);
                self.total_count.fetch_add(count, Ordering::Relaxed);
                Ok(())
            } else {
                self.resize_and_record(value, idx, count)
            }
        }
    }

    #[inline(never)]
    fn resize_and_record(&self, value: u64, idx: u32, count: u64) -> Result<(), RecordError> {
        unsafe {
            self.resize(value)
                .map(|_| {
                    {
                        let _csg = self.wrp.begin_writer_critical_section();
                        let active_counts = &*self.active_counts.load(Ordering::Relaxed);
                        let normalized_index = util::normalize_index(
                            idx,
                            active_counts.normalizing_index_offset(),
                            active_counts.length(),
                        );
                        active_counts
                            .get_unchecked(normalized_index)
                            .fetch_add(count, Ordering::Relaxed); // should probably be consume for arm?
                    }

                    self.update_min_and_max(value);
                    self.total_count.fetch_add(count, Ordering::Relaxed);
                })
                .map_err(|e| RecordError::ResizeFailed(e))
        }
    }

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
        let flip_guard = self.wrp.reader_lock();
        let active_counts = unsafe { &*self.active_counts.load(Ordering::Relaxed) };
        let inactive_counts = unsafe { &*self.inactive_counts.load(Ordering::Relaxed) };

        let new_offset = active_counts.normalizing_index_offset() + offset_to_add;
        if new_offset == active_counts.normalizing_index_offset() {
            return Ok(());
        }

        self.set_normalizing_index_offset_for_inactive(
            inactive_counts,
            new_offset,
            offset_to_add,
            lowest_half_bucket_populated,
        )?;

        self.swap_active_inactive();
        flip_guard.flip();

        let inactive_counts = unsafe { &*self.inactive_counts.load(Ordering::Relaxed) };
        self.set_normalizing_index_offset_for_inactive(
            inactive_counts,
            new_offset,
            offset_to_add,
            lowest_half_bucket_populated,
        )?;

        self.swap_active_inactive();
        flip_guard.flip();

        Ok(())
    }

    fn set_normalizing_index_offset_for_inactive(
        &self,
        inactive_counts: &InlineBackingArray<AtomicU64>,
        new_normalizing_index_offset: i32,
        shifted_amount: i32,
        lowest_half_bucket_populated: bool,
    ) -> Result<(), ShiftError> {
        let pre_shift_zero_index = util::normalize_index(
            0,
            inactive_counts.normalizing_index_offset(),
            inactive_counts.length(),
        );
        let zero_value_count = inactive_counts
            .get_unchecked(pre_shift_zero_index)
            .load(Ordering::Relaxed);
        inactive_counts
            .get_unchecked(pre_shift_zero_index)
            .store(0, Ordering::Relaxed);

        inactive_counts.set_normalizing_index_offset(new_normalizing_index_offset);

        if shifted_amount > 0 && lowest_half_bucket_populated {
            self.shift_lowest_inactive_half_bucket_contents_left(
                inactive_counts,
                shifted_amount as u32,
                pre_shift_zero_index,
            );
        } else if lowest_half_bucket_populated && shifted_amount <= 0 {
            return Err(ShiftError::Underflow);
        }

        let new_zero_index = util::normalize_index(
            0,
            inactive_counts.normalizing_index_offset(),
            inactive_counts.length(),
        );
        inactive_counts
            .get_unchecked(new_zero_index)
            .store(zero_value_count, Ordering::Relaxed);

        Ok(())
    }

    fn shift_lowest_inactive_half_bucket_contents_left(
        &self,
        inactive_counts: &InlineBackingArray<AtomicU64>,
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
                inactive_counts.normalizing_index_offset(),
                inactive_counts.length(),
            );
            let from_normalized_index = from_index + pre_shift_zero_index;
            let count_at_from_index = inactive_counts
                .get_unchecked(from_normalized_index)
                .load(Ordering::Relaxed);
            inactive_counts
                .get_unchecked(normalized_to_index)
                .store(count_at_from_index, Ordering::Relaxed);
            inactive_counts
                .get_unchecked(from_normalized_index)
                .store(0, Ordering::Relaxed);
        }
    }

    pub fn is_auto_resize(&self) -> bool {
        unsafe { (*self.settings.get()).auto_resize }
    }

    pub fn get_count_at_index(&self, index: u32) -> Option<u64> {
        let _g = self.wrp.reader_lock();

        let settings = unsafe { &*self.settings.get() };

        if index >= settings.counts_array_length {
            return None;
        }

        Some(self.unsafe_get_count_at_index(index))
    }

    pub(crate) fn unsafe_get_count_at_index(&self, index: u32) -> u64 {
        unsafe {
            let active_counts = &*self.active_counts.load(Ordering::Acquire);

            let active_index = util::normalize_index(
                index,
                active_counts.normalizing_index_offset(),
                active_counts.length(),
            );
            let inactive_counts = &*self.inactive_counts.load(Ordering::Relaxed);
            let inactive_index = util::normalize_index(
                index,
                inactive_counts.normalizing_index_offset(),
                inactive_counts.length(),
            );
            let active_count = active_counts.get_unchecked(active_index);
            let inactive_count = inactive_counts.get_unchecked(inactive_index);
            active_count.load(Ordering::Relaxed) + inactive_count.load(Ordering::Relaxed)
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

    pub(crate) fn set_integer_to_double_value_conversion_ratio(&self, ratio: f64) {
        let _lg = self.wrp.reader_lock();
        unsafe {
            let settings = &mut *self.settings.get();
            settings.integer_to_double_value_conversion_ratio = ratio;
            settings.double_to_integer_value_conversion_ratio = 1.0 / ratio;
        }
    }

    unsafe fn copy_counts(&self, source: &InlineBackingArray<AtomicU64>, target: &mut InlineBackingArray<AtomicU64>) {
        ptr::copy_nonoverlapping(
            source.get_array_ptr(),
            (*target).get_array_ptr(),
            self.counts_array_length() as usize,
        );
        target.set_normalizing_index_offset(source.normalizing_index_offset());
    }

    unsafe fn copy_inactive_counts_contents_on_resize(
        &self,
        old_inactive: &InlineBackingArray<AtomicU64>,
        new_inactive: &InlineBackingArray<AtomicU64>,
        counts_delta: u32,
    ) {
        let old_zero_index = util::normalize_index(
            0,
            old_inactive.normalizing_index_offset(),
            old_inactive.length(),
        );
        new_inactive.set_normalizing_index_offset(old_inactive.normalizing_index_offset());

        if old_zero_index == 0 {
            for i in 0..old_inactive.length() {
                let value = old_inactive.get_unchecked(i).load(Ordering::Relaxed);
                new_inactive.get_unchecked(i).store(value, Ordering::Relaxed);
            }
        } else {
            for i in 0..old_zero_index {
                let value = old_inactive.get_unchecked(i).load(Ordering::Relaxed);
                new_inactive.get_unchecked(i).store(value, Ordering::Relaxed);
            }
            for i in old_zero_index..old_inactive.length() {
                let value = old_inactive.get_unchecked(i).load(Ordering::Relaxed);
                new_inactive
                    .get_unchecked(i + counts_delta)
                    .store(value, Ordering::Relaxed);
            }
        }
    }

    fn swap_active_inactive(&self) {
        let active_ptr = self.active_counts.load(Ordering::Relaxed);
        let inactive_ptr = self.inactive_counts.load(Ordering::Relaxed);
        self.active_counts.store(inactive_ptr, Ordering::SeqCst);
        self.inactive_counts.store(active_ptr, Ordering::SeqCst);
    }

    #[inline(never)]
    pub fn resize(&self, new_highest_trackable_value: u64) -> Result<(), CreationError> {
        unsafe {
            let flip_guard = self.wrp.reader_lock();

            let settings = &mut *self.settings.get();

            let active_counts = &*self.active_counts.load(Ordering::Relaxed);
            let inactive_counts = &*self.inactive_counts.load(Ordering::Relaxed);

            assert!(settings.counts_array_length == active_counts.length());
            assert!(settings.counts_array_length == inactive_counts.length());

            let new_array_length = settings.determine_array_length_needed(new_highest_trackable_value);
            if new_array_length > i32::MAX as u32 {
                return Err(CreationError::RequiresExcessiveArrayLen);
            }

            let counts_delta = new_array_length - settings.counts_array_length;
            if counts_delta == 0 {
                return Ok(());
            }

            let new_inactive_counts_1 = InlineBackingArray::<AtomicU64>::new(new_array_length);
            let new_inactive_counts_2 = InlineBackingArray::<AtomicU64>::new(new_array_length);

            let previous_inactive_counts = self.inactive_counts.load(Ordering::Relaxed);
            self.inactive_counts.store(new_inactive_counts_1, Ordering::SeqCst);
            self.copy_inactive_counts_contents_on_resize(
                &*previous_inactive_counts,
                &*new_inactive_counts_1,
                counts_delta,
            );

            self.swap_active_inactive();
            flip_guard.flip();

            let previous_active_counts = self.inactive_counts.load(Ordering::Relaxed);
            self.inactive_counts.store(new_inactive_counts_2, Ordering::SeqCst);
            self.copy_inactive_counts_contents_on_resize(
                &*previous_active_counts,
                &*new_inactive_counts_2,
                counts_delta,
            );

            self.swap_active_inactive();
            flip_guard.flip();

            (*previous_active_counts).dealloc();
            (*previous_inactive_counts).dealloc();

            settings.resize(new_highest_trackable_value)?;

            assert!(settings.counts_array_length == (*self.active_counts.load(Ordering::Relaxed)).length());
            assert!(settings.counts_array_length == (*self.inactive_counts.load(Ordering::Relaxed)).length());

            Ok(())
        }
    }

    pub fn set_auto_resize(&self, resize: bool) {
        unsafe {
            let _lg = self.wrp.reader_lock();
            (*self.settings.get()).auto_resize = resize;
        }
    }

    pub unsafe fn clear_counts(&self) {
        let _lg = self.wrp.reader_lock();
        let active_counts = self.active_counts.load(Ordering::Relaxed);
        let inactive_counts = self.inactive_counts.load(Ordering::Relaxed);
        let settings = &*self.settings.get();
        for ix in 0..settings.counts_array_length {
            (*active_counts)
                .get_unchecked(ix)
                .store(0, Ordering::Relaxed);
            (*inactive_counts)
                .get_unchecked(ix)
                .store(0, Ordering::Relaxed);
        }
        self.total_count.store(0, Ordering::Relaxed);
        self.raw_max_value
            .store(ORIGINAL_MAX | settings.unit_magnitude_mask, Ordering::Relaxed);
        self.raw_min_non_zero_value
            .store(ORIGINAL_MIN, Ordering::Relaxed);
        let meta_data = &self.meta_data as *const HistogramMetaData as *mut HistogramMetaData;
        (*meta_data).clear();
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
            let inactive_counts = unsafe { &*self.inactive_counts.load(Ordering::Relaxed) };
            let other_inactive_counts = unsafe { &*other.inactive_counts.load(Ordering::Relaxed) };
            for i in 0..self_len {
                check_eq!(
                    self.unsafe_get_count_at_index(i),
                    other.unsafe_get_count_at_index(i)
                );
                let inactive_index = util::normalize_index(
                    i,
                    inactive_counts.normalizing_index_offset(),
                    inactive_counts.length(),
                );
                let other_inactive_index = util::normalize_index(
                    i,
                    other_inactive_counts.normalizing_index_offset(),
                    other_inactive_counts.length(),
                );
                check_eq!(
                    inactive_counts.get_unchecked(inactive_index).load(Ordering::Relaxed),
                    other_inactive_counts
                        .get_unchecked(other_inactive_index)
                        .load(Ordering::Relaxed)
                );
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

    pub(in concurrent) fn write_inactive_to_active(&self) { unsafe {
        let _lg = self.wrp.reader_lock();
        let active_counts = self.active_counts.load(Ordering::Relaxed);
        let inactive_counts = self.inactive_counts.load(Ordering::Relaxed);
        for idx in 0..self.settings().counts_array_length {
            let active_index = util::normalize_index(
                idx,
                (*active_counts).normalizing_index_offset(),
                (*active_counts).length(),
            );
            let inactive_index = util::normalize_index(
                idx,
                (*inactive_counts).normalizing_index_offset(),
                (*inactive_counts).length(),
            );
            let inactive_loc = (*inactive_counts).get_unchecked(inactive_index);
            let count = inactive_loc.load(Ordering::Relaxed);
            (*active_counts)
                .get_unchecked(active_index)
                .fetch_add(count, Ordering::Relaxed);
            inactive_loc.store(0, Ordering::Relaxed);
        }
    }}

    pub unsafe fn unsafe_as_snapshot(&self) -> Snapshot<Self> {
        #[allow(mutable_transmutes)]
        Snapshot::new(mem::transmute(self))
    }

    pub fn as_snapshot(&mut self) -> Snapshot<Self> {
        unsafe { Snapshot::new(self) }
    }
}

impl ConstructableHistogram for ResizableHistogram {
    fn new(lowest_discernible_value: u64, highest_trackable_value: u64, significant_value_digits: u8) -> Result<Self, CreationError> {
        ResizableHistogram::with_low_high_sigvdig(
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

impl RecordableHistogram for ResizableHistogram {
    fn fresh(settings: &HistogramSettings) -> Result<ResizableHistogram, CreationError> {
        let lowest_discernable = settings.lowest_discernible_value;
        let highest_trackable = settings.highest_trackable_value;
        let sigvdig = settings.number_of_significant_value_digits as u8;
        ResizableHistogram::with_low_high_sigvdig(lowest_discernable, highest_trackable, sigvdig)
    }
    #[inline(always)]
    fn meta_data_mut(&mut self) -> &mut HistogramMetaData {
        &mut self.meta_data
    }
    #[inline(always)]
    unsafe fn clear_counts(&self) {
        ResizableHistogram::clear_counts(self);
    }
    fn equals(&mut self, other: &mut Self) -> bool {
        ResizableHistogram::equals(self, other)
    }
    #[inline(always)]
    fn record_value(&self, value: u64) -> Result<(), RecordError> {
        ResizableHistogram::record_value(self, value)
    }
    #[inline(always)]
    fn record_value_with_count(&self, value: u64, count: u64) -> Result<(), RecordError> {
        ResizableHistogram::record_value_with_count(self, value, count)
    }
}

impl ReadableHistogram for ResizableHistogram {
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
        self.settings()
            .get_max_value(self.raw_max_value.load(Ordering::Relaxed))
    }
    fn meta_data(&self) -> &HistogramMetaData { &self.meta_data }
}
