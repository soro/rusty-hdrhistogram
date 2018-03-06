use concurrent::{concurrent_util, Snapshot};
use concurrent::inline_backing_array::InlineBackingArray;
use concurrent::recordable_histogram::RecordableHistogram;
use core::*;
use core::constants::*;
use std::{heap, mem, ptr, slice};
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicPtr, AtomicU64};
use std::sync::atomic::Ordering;

#[repr(C)]
pub struct StaticHistogram {
    meta_data: HistogramMetaData,
    settings: UnsafeCell<HistogramSettings>,
    raw_max_value: AtomicU64,
    raw_min_non_zero_value: AtomicU64,
    total_count: AtomicU64,
    pub(in concurrent) counts: AtomicPtr<InlineBackingArray<AtomicU64>>,
}

impl StaticHistogram {
    pub fn new(highest_trackable_value: u64, significant_value_digits: u8) -> Result<StaticHistogram, CreationError> {
        StaticHistogram::with_low_high_sigvdig(1, highest_trackable_value, significant_value_digits)
    }
    pub fn with_low_high_sigvdig(
        lowest_discernible_value: u64,
        highest_trackable_value: u64,
        significant_value_digits: u8,
    ) -> Result<StaticHistogram, CreationError> {
        let settings = HistogramSettings::new(
            lowest_discernible_value,
            highest_trackable_value,
            significant_value_digits,
        )?;
        unsafe {
            let array_ptr = InlineBackingArray::new_in(settings.counts_array_length, heap::Heap);
            Ok(StaticHistogram {
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
                let c = counts.get_unchecked(idx);
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
        concurrent_util::update_max_value(&self.settings, &self.raw_max_value, value);
    }

    #[inline]
    fn update_min_non_zero_value(&self, value: u64) {
        concurrent_util::update_min_non_zero_value(&self.settings, &self.raw_min_non_zero_value, value);
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
                Some(counts.get_unchecked(index).load(Ordering::Relaxed))
            }
        }
    }

    pub(crate) fn unsafe_get_count_at_index(&self, index: u32) -> u64 {
        unsafe {
            let counts = &*self.counts.load(Ordering::Relaxed);

            counts.get_unchecked(index).load(Ordering::Relaxed)
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
        for i in 0..self.counts_array_length() {
            (*counts).get_unchecked(i).store(0, Ordering::Relaxed);
        }
        self.total_count.store(0, Ordering::Relaxed);
    }

    unsafe fn copy_counts(&self, source: &InlineBackingArray<AtomicU64>, target: &mut InlineBackingArray<AtomicU64>) {
        ptr::copy_nonoverlapping(
            source.get_array_ptr(),
            (*target).get_array_ptr(),
            self.counts_array_length() as usize,
        );
    }

    fn equals(&mut self, other: &mut Self) -> bool {
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
        for i in 0..self.counts_array_length() {
            check_eq!(
                self.unsafe_get_count_at_index(i),
                other.unsafe_get_count_at_index(i)
            )
        }
        return true;
    }

    // this is really, really unsafe. use only if you can guarantee unique access
    pub unsafe fn unsafe_as_snapshot(&self) -> Snapshot<Self> {
        #[allow(mutable_transmutes)]
        Snapshot::new(mem::transmute(self))
    }

    pub fn as_snapshot(&mut self) -> Snapshot<Self> {
        unsafe { Snapshot::new(self) }
    }
}

impl ConstructableHistogram for StaticHistogram {
    fn new(lowest_discernible_value: u64, highest_trackable_value: u64, significant_value_digits: u8) -> Result<Self, CreationError> {
        StaticHistogram::with_low_high_sigvdig(
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
            ORIGINAL_MIN & !self.settings().unit_magnitude_mask,
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

impl RecordableHistogram for StaticHistogram {
    fn fresh(settings: &HistogramSettings) -> Result<StaticHistogram, CreationError> {
        let lowest_discernable = settings.lowest_discernible_value;
        let highest_trackable = settings.highest_trackable_value;
        let sigvdig = settings.number_of_significant_value_digits as u8;
        StaticHistogram::with_low_high_sigvdig(lowest_discernable, highest_trackable, sigvdig)
    }
    #[inline(always)]
    fn meta_data_mut(&mut self) -> &mut HistogramMetaData {
        &mut self.meta_data
    }
    #[inline(always)]
    unsafe fn clear_counts(&self) {
        StaticHistogram::clear_counts(self);
    }
    fn equals(&mut self, other: &mut Self) -> bool {
        StaticHistogram::equals(self, other)
    }
    #[inline(always)]
    fn record_value(&self, value: u64) -> Result<(), RecordError> {
        StaticHistogram::record_value(self, value)
    }
    #[inline(always)]
    fn record_value_with_count(&self, value: u64, count: u64) -> Result<(), RecordError> {
        StaticHistogram::record_value_with_count(self, value, count)
    }
}

impl MutSliceableHistogram<u64> for StaticHistogram {
    fn get_counts_slice_mut<'b>(&'b mut self, length: u32) -> Option<&'b mut [u64]> {
        unsafe {
            let counts = self.counts.load(Ordering::Relaxed);
            if length <= (*counts).length() {
                return Some(slice::from_raw_parts_mut(
                    mem::transmute((*counts).get_array_ptr()),
                    length as usize,
                ));
            }
            None
        }
    }
}

impl ReadableHistogram for StaticHistogram {
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
        StaticHistogram::get_max_value(self)
    }
    fn meta_data(&self) -> &HistogramMetaData { &self.meta_data }
}
