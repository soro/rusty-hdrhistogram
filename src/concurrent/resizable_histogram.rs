use concurrent::{concurrent_util, Snapshot, WriterReaderPhaser};
use concurrent::inline_backing_array::InlineBackingArray;
use concurrent::recordable_histogram::RecordableHistogram;
use core::*;
use core::constants::*;
use std::{heap, mem, ptr, slice};
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
            let active_array_ptr = InlineBackingArray::new_in(settings.counts_array_length, heap::Heap);
            let inactive_array_ptr = InlineBackingArray::new_in(settings.counts_array_length, heap::Heap);
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
    fn record_count_at_value(&self, count: u64, value: u64) -> Result<(), RecordError> {
        unsafe {
            let idx = (*self.settings.get()).counts_array_index(value);

            if idx < (*self.settings.get()).counts_array_length {
                {
                    let _csg = self.wrp.begin_writer_critical_section();
                    let active_counts = &*self.active_counts.load(Ordering::Relaxed);
                    let c = active_counts.get_unchecked(idx);
                    c.fetch_add(count, Ordering::Relaxed);
                }
                self.update_min_and_max(value);
                self.total_count.fetch_add(count, Ordering::Relaxed);
                Ok(())
            } else if !self.is_auto_resize() {
                Err(RecordError::ValueOutOfRangeResizeDisabled)
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
                        active_counts
                            .get_unchecked(idx)
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

            let active_count = active_counts.get_unchecked(index);
            let inactive_count = (*self.inactive_counts.load(Ordering::Relaxed)).get_unchecked(index);
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

    unsafe fn copy_counts(&self, source: &InlineBackingArray<AtomicU64>, target: &mut InlineBackingArray<AtomicU64>) {
        ptr::copy_nonoverlapping(
            source.get_array_ptr(),
            (*target).get_array_ptr(),
            self.counts_array_length() as usize,
        );
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
            // TODO: this should be a global length limit enforced throughout allocation etc
            if new_array_length > i32::max_value() as u32 {
                return Err(CreationError::RequiresExcessiveArrayLen);
            }

            let counts_delta = new_array_length as isize - settings.counts_array_length as isize;

            if counts_delta <= 0 {
                return Ok(());
            }

            // TODO: all of normalization
            let new_inactive_counts = InlineBackingArray::<AtomicU64>::new_in(new_array_length, heap::Heap);
            let previous_inactive_counts = self.inactive_counts.load(Ordering::Relaxed);
            self.copy_counts(&*previous_inactive_counts, &mut *new_inactive_counts);
            // TODO: add shifting stuff here - maybe already at copy into

            let previous_active_counts = self.active_counts
                .swap(new_inactive_counts, Ordering::SeqCst);

            flip_guard.flip();

            let resized_previous_active_counts = InlineBackingArray::<AtomicU64>::new_in(new_array_length, heap::Heap);
            self.copy_counts(
                &*previous_active_counts,
                &mut *resized_previous_active_counts,
            );
            // TODO: add shifting stuff here as well

            let resized_inactive_counts = self.active_counts
                .swap(resized_previous_active_counts, Ordering::SeqCst);
            self.inactive_counts
                .store(resized_inactive_counts, Ordering::SeqCst);

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
        for ix in 0..(*self.settings.get()).counts_array_length {
            (*active_counts)
                .get_unchecked(ix)
                .store(0, Ordering::Relaxed);
            (*inactive_counts)
                .get_unchecked(ix)
                .store(0, Ordering::Relaxed);
        }
        self.total_count.store(0, Ordering::Relaxed);
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
        let inactive_counts = self.inactive_counts.load(Ordering::Relaxed);
        let other_inactive_counts = other.inactive_counts.load(Ordering::Relaxed);
        for i in 0..self.counts_array_length() {
            check_eq!(
                self.unsafe_get_count_at_index(i),
                other.unsafe_get_count_at_index(i)
            );
            unsafe {
                check_eq!(
                    (*inactive_counts).get_unchecked(i).load(Ordering::Relaxed),
                    (*other_inactive_counts).get_unchecked(i).load(Ordering::Relaxed)
                )
            }
        }
        return true;
    }

    pub(in concurrent) fn write_inactive_to_active(&self) { unsafe {
        let _lg = self.wrp.reader_lock();
        let active_counts = self.active_counts.load(Ordering::Relaxed);
        let inactive_counts = self.inactive_counts.load(Ordering::Relaxed);
        for idx in 0..self.settings().counts_array_length {
            let inactive_loc = (*inactive_counts).get_unchecked(idx);
            let count = inactive_loc.load(Ordering::Relaxed);
            (*active_counts).get_unchecked(idx).fetch_add(count, Ordering::Relaxed);
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

impl MutSliceableHistogram<u64> for ResizableHistogram {
    fn get_counts_slice_mut<'b>(&'b mut self, length: u32) -> Option<&'b mut [u64]> { unsafe {
        if length > self.settings().counts_array_length { return None; }
        else {
            let active_counts = self.active_counts.load(Ordering::Relaxed);
            if self.get_total_count() != 0 {
                self.write_inactive_to_active();
            }
            return Some(slice::from_raw_parts_mut(mem::transmute((*active_counts).get_array_ptr()), length as usize));
        }
    }}
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
