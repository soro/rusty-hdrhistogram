use crate::concurrent::concurrent_util;
use crate::concurrent::inline_backing_array::InlineBackingArray;
use crate::concurrent::recordable_histogram::RecordableHistogram;
use crate::concurrent::snapshot::{ResizableSnapshot, Snapshot};
use crate::concurrent::writer_reader_phaser::{PhaseFlipGuard, WriterReaderPhaser};
use crate::core::constants::*;
use crate::core::*;
use crate::iteration::{AllValuesIterator, LinearIterator, LogarithmicIterator, PercentileIterator, RecordedValuesIterator};
use crossbeam_epoch as epoch;
use std::ptr;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::atomic::{AtomicPtr, AtomicU64};

#[repr(C)]
pub struct ResizableConcurrentHistogram {
    meta_data: HistogramMetaData,
    layout: HistogramLayout,
    auto_resize: AtomicBool,
    wrp: WriterReaderPhaser,
    raw_max_value: AtomicU64,
    raw_min_non_zero_value: AtomicU64,
    total_count: AtomicU64,
    pub(in crate::concurrent) active_counts: AtomicPtr<InlineBackingArray<AtomicU64>>,
    inactive_counts: AtomicPtr<InlineBackingArray<AtomicU64>>,
}

/// A structurally stable view of a [`ResizableConcurrentHistogram`].
///
/// The view captures the active/inactive backing arrays, storage metadata, and
/// range metadata from one structural epoch. Ordinary recording can still update
/// count cells while the view is alive, so this is not a frozen count snapshot:
/// per-bucket counts may reflect writes that happened after the captured
/// `total_count`, min, and max values. Use recorder samples/snapshots when a
/// stable count set is required.
///
/// While alive, the view holds the histogram's structural read side and can
/// delay resize or double range-shift operations. Do not hold a view while
/// calling other methods on the same histogram that may need that same
/// structural lock.
pub struct ResizableConcurrentReadView<'a> {
    histogram: &'a ResizableConcurrentHistogram,
    _phase_guard: PhaseFlipGuard<'a>,
    _epoch_guard: epoch::Guard,
    active_counts: *const InlineBackingArray<AtomicU64>,
    inactive_counts: *const InlineBackingArray<AtomicU64>,
    settings: HistogramSettings,
    active_normalizing_index_offset: i32,
    inactive_normalizing_index_offset: i32,
    integer_to_double_value_conversion_ratio: f64,
    total_count: u64,
    max_value: u64,
    min_non_zero_value: u64,
}

pub(crate) struct ResizableStructuralMutation<'a> {
    histogram: &'a ResizableConcurrentHistogram,
    guard: PhaseFlipGuard<'a>,
}

impl Drop for ResizableConcurrentHistogram {
    fn drop(&mut self) {
        unsafe {
            self.wrp.reader_lock().flip();
            (*self.active_counts.load(Ordering::SeqCst)).dealloc();
            (*self.inactive_counts.load(Ordering::SeqCst)).dealloc();
        }
    }
}

unsafe impl Send for ResizableConcurrentHistogram {}
unsafe impl Sync for ResizableConcurrentHistogram {}

impl ResizableConcurrentHistogram {
    pub fn new(significant_value_digits: u8) -> Result<ResizableConcurrentHistogram, CreationError> {
        ResizableConcurrentHistogram::with_sigvdig(significant_value_digits)
    }
    pub fn with_sigvdig(significant_value_digits: u8) -> Result<ResizableConcurrentHistogram, CreationError> {
        ResizableConcurrentHistogram::with_high_sigvdig(2, significant_value_digits)
    }
    pub fn with_high_sigvdig(
        highest_trackable_value: u64,
        significant_value_digits: u8,
    ) -> Result<ResizableConcurrentHistogram, CreationError> {
        ResizableConcurrentHistogram::with_low_high_sigvdig(1, highest_trackable_value, significant_value_digits)
    }
    pub fn with_low_high_sigvdig(
        lowest_discernible_value: u64,
        highest_trackable_value: u64,
        significant_value_digits: u8,
    ) -> Result<ResizableConcurrentHistogram, CreationError> {
        let layout = HistogramLayout::new(lowest_discernible_value, highest_trackable_value, significant_value_digits)?;
        let metadata = layout.initial_metadata_for_highest(highest_trackable_value)?;
        unsafe {
            let active_array_ptr = InlineBackingArray::new(metadata);
            let inactive_array_ptr = InlineBackingArray::new(metadata);
            Ok(ResizableConcurrentHistogram {
                meta_data: HistogramMetaData::new(),
                wrp: WriterReaderPhaser::new(),
                layout,
                auto_resize: AtomicBool::new(true),
                raw_max_value: AtomicU64::new(ORIGINAL_MAX),
                raw_min_non_zero_value: AtomicU64::new(ORIGINAL_MIN),
                total_count: AtomicU64::new(0),
                active_counts: AtomicPtr::new(active_array_ptr),
                inactive_counts: AtomicPtr::new(inactive_array_ptr),
            })
        }
    }

    fn active_counts<'g>(&self, _guard: &'g epoch::Guard, ordering: Ordering) -> &'g InlineBackingArray<AtomicU64> {
        unsafe { &*self.active_counts.load(ordering) }
    }

    unsafe fn retire_counts_array(ptr: *mut InlineBackingArray<AtomicU64>, guard: &epoch::Guard) {
        unsafe {
            guard.defer_unchecked(move || {
                (*ptr).dealloc();
            });
        }
    }

    pub(crate) fn begin_structural_mutation(&self) -> ResizableStructuralMutation<'_> {
        ResizableStructuralMutation {
            histogram: self,
            guard: self.wrp.reader_lock(),
        }
    }

    pub(crate) fn lowest_tracking_integer_value(&self) -> u64 {
        self.layout.sub_bucket_half_count as u64
    }

    /// Capture a structurally stable read view.
    ///
    /// The returned view keeps the backing arrays it references alive and keeps
    /// their storage metadata consistent with count-index mapping. It does not
    /// freeze ordinary count updates; concurrent recorders may still change
    /// bucket values while the view is encoded or queried.
    pub fn read_view(&self) -> ResizableConcurrentReadView<'_> {
        let phase_guard = self.wrp.reader_lock();
        let epoch_guard = epoch::pin();
        unsafe {
            let active_counts = &*self.active_counts.load(Ordering::Acquire);
            let inactive_counts = &*self.inactive_counts.load(Ordering::Relaxed);
            debug_assert_eq!(active_counts.length(), inactive_counts.length());
            let settings = self
                .layout
                .settings_snapshot(active_counts.metadata(), self.auto_resize.load(Ordering::Relaxed));
            ResizableConcurrentReadView {
                histogram: self,
                _phase_guard: phase_guard,
                _epoch_guard: epoch_guard,
                active_counts,
                inactive_counts,
                settings,
                active_normalizing_index_offset: active_counts.normalizing_index_offset(),
                inactive_normalizing_index_offset: inactive_counts.normalizing_index_offset(),
                integer_to_double_value_conversion_ratio: active_counts.integer_to_double_value_conversion_ratio(),
                total_count: self.total_count.load(Ordering::Relaxed),
                max_value: self.layout.get_max_value(self.raw_max_value.load(Ordering::Relaxed)),
                min_non_zero_value: self
                    .layout
                    .get_min_non_zero_value(self.raw_min_non_zero_value.load(Ordering::Relaxed)),
            }
        }
    }

    pub fn counts_array_length(&self) -> u32 {
        let _g = self.wrp.reader_lock();
        let guard = epoch::pin();
        self.active_counts(&guard, Ordering::Acquire).length()
    }

    pub fn settings(&self) -> HistogramSettings {
        let _g = self.wrp.reader_lock();
        let guard = epoch::pin();
        let metadata = self.active_counts(&guard, Ordering::Acquire).metadata();
        self.layout.settings_snapshot(metadata, self.auto_resize.load(Ordering::Relaxed))
    }

    pub(crate) fn normalizing_index_offset(&self) -> i32 {
        let _g = self.wrp.reader_lock();
        let guard = epoch::pin();
        self.active_counts(&guard, Ordering::Acquire).normalizing_index_offset()
    }

    pub(crate) fn integer_to_double_value_conversion_ratio(&self) -> f64 {
        let _g = self.wrp.reader_lock();
        let guard = epoch::pin();
        self.active_counts(&guard, Ordering::Acquire)
            .integer_to_double_value_conversion_ratio()
    }

    pub(crate) fn double_to_integer_value_conversion_ratio(&self) -> f64 {
        let _g = self.wrp.reader_lock();
        let guard = epoch::pin();
        self.active_counts(&guard, Ordering::Acquire)
            .double_to_integer_value_conversion_ratio()
    }

    pub(crate) fn set_normalizing_index_offset(&self, offset: i32) {
        let _lg = self.wrp.reader_lock();
        unsafe {
            let active_counts = &*self.active_counts.load(Ordering::Acquire);
            let inactive_counts = &*self.inactive_counts.load(Ordering::Relaxed);
            active_counts.set_normalizing_index_offset(offset);
            inactive_counts.set_normalizing_index_offset(offset);
        }
    }

    pub(crate) fn set_count_at_index(&self, index: u32, count: u64) {
        let _lg = self.wrp.reader_lock();
        unsafe {
            let active_counts = &*self.active_counts.load(Ordering::Acquire);
            let normalized_index = util::normalize_index(index, active_counts.normalizing_index_offset(), active_counts.length());
            active_counts.get_unchecked(normalized_index).store(count, Ordering::Relaxed);
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
    pub(crate) fn record_value_with_count_strict(&self, value: u64, count: u64) -> Result<(), RecordError> {
        unsafe {
            let idx = self.layout.counts_array_index(value);
            {
                let _csg = self.wrp.begin_writer_critical_section();
                let active_counts = &*self.active_counts.load(Ordering::Acquire);
                if idx >= active_counts.length() {
                    return Err(RecordError::ValueOutOfRangeResizeDisabled);
                }
                let normalized_index = util::normalize_index(idx, active_counts.normalizing_index_offset(), active_counts.length());
                let c = active_counts.get_unchecked(normalized_index);
                c.fetch_add(count, Ordering::Relaxed);
            }
            self.update_min_and_max(value);
            self.total_count.fetch_add(count, Ordering::Relaxed);
            Ok(())
        }
    }

    #[inline(always)]
    pub(crate) fn record_value_with_count_strict_guarded(
        &self,
        value: u64,
        count: u64,
        range_shift_in_progress: &AtomicBool,
    ) -> Result<bool, RecordError> {
        unsafe {
            let idx = self.layout.counts_array_index(value);
            {
                let _csg = self.wrp.begin_writer_critical_section();
                if range_shift_in_progress.load(Ordering::SeqCst) {
                    return Ok(false);
                }
                let active_counts = &*self.active_counts.load(Ordering::Acquire);
                if idx >= active_counts.length() {
                    return Err(RecordError::ValueOutOfRangeResizeDisabled);
                }
                let normalized_index = util::normalize_index(idx, active_counts.normalizing_index_offset(), active_counts.length());
                let c = active_counts.get_unchecked(normalized_index);
                c.fetch_add(count, Ordering::Relaxed);
                self.update_min_and_max(value);
                self.total_count.fetch_add(count, Ordering::Relaxed);
            }
            Ok(true)
        }
    }

    #[inline(always)]
    fn record_count_at_value(&self, count: u64, value: u64) -> Result<(), RecordError> {
        unsafe {
            let idx = self.layout.counts_array_index(value);

            {
                let _csg = self.wrp.begin_writer_critical_section();
                let active_counts = &*self.active_counts.load(Ordering::Acquire);
                if idx < active_counts.length() {
                    let normalized_index = util::normalize_index(idx, active_counts.normalizing_index_offset(), active_counts.length());
                    active_counts.get_unchecked(normalized_index).fetch_add(count, Ordering::Relaxed);
                    self.update_min_and_max(value);
                    self.total_count.fetch_add(count, Ordering::Relaxed);
                    return Ok(());
                }
            }

            if !self.is_auto_resize() {
                Err(RecordError::ValueOutOfRangeResizeDisabled)
            } else {
                self.resize_and_record(value, idx, count)
            }
        }
    }

    #[inline(always)]
    pub(crate) fn record_value_with_count_saturating(&self, value: u64, count: u64) -> Result<(), RecordError> {
        unsafe {
            let recorded_value;
            {
                let _csg = self.wrp.begin_writer_critical_section();
                let active_counts = &*self.active_counts.load(Ordering::Acquire);
                let idx = self.layout.counts_array_index(value).min(active_counts.length() - 1);
                recorded_value = self.layout.value_from_index(idx);
                let normalized_index = util::normalize_index(idx, active_counts.normalizing_index_offset(), active_counts.length());
                let c = active_counts.get_unchecked(normalized_index);
                c.fetch_add(count, Ordering::Relaxed);
            }
            self.update_min_and_max(recorded_value);
            self.total_count.fetch_add(count, Ordering::Relaxed);
            Ok(())
        }
    }

    #[inline(always)]
    pub(crate) fn record_converted_double_value_with_count(&self, value: f64, count: u64) -> Result<(), RecordError> {
        unsafe {
            {
                let _csg = self.wrp.begin_writer_critical_section();
                let active_counts = &*self.active_counts.load(Ordering::Acquire);
                let integer_value = Self::to_integer_value(value, active_counts.double_to_integer_value_conversion_ratio())?;
                let idx = self.layout.counts_array_index(integer_value);
                if idx >= active_counts.length() {
                    return Err(RecordError::ValueOutOfRangeResizeDisabled);
                }
                let normalized_index = util::normalize_index(idx, active_counts.normalizing_index_offset(), active_counts.length());
                let c = active_counts.get_unchecked(normalized_index);
                c.fetch_add(count, Ordering::Relaxed);
                self.update_min_and_max(integer_value);
                self.total_count.fetch_add(count, Ordering::Relaxed);
            }
            Ok(())
        }
    }

    #[inline(always)]
    pub(crate) fn record_converted_double_value_with_count_guarded(
        &self,
        value: f64,
        count: u64,
        range_shift_in_progress: &AtomicBool,
        range_generation: &AtomicU64,
        expected_range_generation: u64,
    ) -> Result<bool, RecordError> {
        unsafe {
            {
                let _csg = self.wrp.begin_writer_critical_section();
                if range_shift_in_progress.load(Ordering::SeqCst) {
                    return Ok(false);
                }
                if range_generation.load(Ordering::Acquire) != expected_range_generation {
                    return Ok(false);
                }
                let active_counts = &*self.active_counts.load(Ordering::Acquire);
                let integer_value = Self::to_integer_value(value, active_counts.double_to_integer_value_conversion_ratio())?;
                let idx = self.layout.counts_array_index(integer_value);
                if idx >= active_counts.length() {
                    return Err(RecordError::ValueOutOfRangeResizeDisabled);
                }
                let normalized_index = util::normalize_index(idx, active_counts.normalizing_index_offset(), active_counts.length());
                let c = active_counts.get_unchecked(normalized_index);
                c.fetch_add(count, Ordering::Relaxed);
                self.update_min_and_max(integer_value);
                self.total_count.fetch_add(count, Ordering::Relaxed);
            }
            Ok(true)
        }
    }

    #[inline(always)]
    pub(crate) fn record_in_range_converted_double_value_guarded(
        &self,
        value: f64,
        range_shift_in_progress: &AtomicBool,
        range_generation: &AtomicU64,
        expected_range_generation: u64,
    ) -> Result<bool, RecordError> {
        unsafe {
            {
                let _csg = self.wrp.begin_writer_critical_section();
                if range_shift_in_progress.load(Ordering::SeqCst) {
                    return Ok(false);
                }
                if range_generation.load(Ordering::Acquire) != expected_range_generation {
                    return Ok(false);
                }
                let active_counts = &*self.active_counts.load(Ordering::Acquire);
                let integer_value = Self::to_integer_value_unchecked(value, active_counts.double_to_integer_value_conversion_ratio());
                let idx = self.layout.counts_array_index(integer_value);
                if idx >= active_counts.length() {
                    return Err(RecordError::ValueOutOfRangeResizeDisabled);
                }
                let normalized_index = util::normalize_index(idx, active_counts.normalizing_index_offset(), active_counts.length());
                let c = active_counts.get_unchecked(normalized_index);
                c.fetch_add(1, Ordering::Relaxed);
                self.update_min_and_max(integer_value);
                self.total_count.fetch_add(1, Ordering::Relaxed);
            }
            Ok(true)
        }
    }

    #[inline(always)]
    pub(crate) fn record_converted_double_value_with_count_saturating(&self, value: f64, count: u64) -> Result<(), RecordError> {
        unsafe {
            {
                let _csg = self.wrp.begin_writer_critical_section();
                let active_counts = &*self.active_counts.load(Ordering::Acquire);
                let integer_value = Self::to_integer_value_saturating(value, active_counts.double_to_integer_value_conversion_ratio());
                let idx = self.layout.counts_array_index(integer_value).min(active_counts.length() - 1);
                let recorded_value = self.layout.value_from_index(idx);
                let normalized_index = util::normalize_index(idx, active_counts.normalizing_index_offset(), active_counts.length());
                let c = active_counts.get_unchecked(normalized_index);
                c.fetch_add(count, Ordering::Relaxed);
                self.update_min_and_max(recorded_value);
                self.total_count.fetch_add(count, Ordering::Relaxed);
            }
            Ok(())
        }
    }

    #[inline(always)]
    pub(crate) fn record_converted_double_value_with_count_saturating_guarded(
        &self,
        value: f64,
        count: u64,
        range_shift_in_progress: &AtomicBool,
        range_generation: &AtomicU64,
        expected_range_generation: u64,
    ) -> Result<bool, RecordError> {
        unsafe {
            {
                let _csg = self.wrp.begin_writer_critical_section();
                if range_shift_in_progress.load(Ordering::SeqCst) {
                    return Ok(false);
                }
                if range_generation.load(Ordering::Acquire) != expected_range_generation {
                    return Ok(false);
                }
                let active_counts = &*self.active_counts.load(Ordering::Acquire);
                let integer_value = Self::to_integer_value_saturating(value, active_counts.double_to_integer_value_conversion_ratio());
                let idx = self.layout.counts_array_index(integer_value).min(active_counts.length() - 1);
                let recorded_value = self.layout.value_from_index(idx);
                let normalized_index = util::normalize_index(idx, active_counts.normalizing_index_offset(), active_counts.length());
                let c = active_counts.get_unchecked(normalized_index);
                c.fetch_add(count, Ordering::Relaxed);
                self.update_min_and_max(recorded_value);
                self.total_count.fetch_add(count, Ordering::Relaxed);
            }
            Ok(true)
        }
    }

    #[inline(always)]
    pub(crate) fn record_in_range_converted_double_value_saturating_guarded(
        &self,
        value: f64,
        range_shift_in_progress: &AtomicBool,
        range_generation: &AtomicU64,
        expected_range_generation: u64,
    ) -> Result<bool, RecordError> {
        unsafe {
            {
                let _csg = self.wrp.begin_writer_critical_section();
                if range_shift_in_progress.load(Ordering::SeqCst) {
                    return Ok(false);
                }
                if range_generation.load(Ordering::Acquire) != expected_range_generation {
                    return Ok(false);
                }
                let active_counts = &*self.active_counts.load(Ordering::Acquire);
                let integer_value = Self::to_integer_value_unchecked(value, active_counts.double_to_integer_value_conversion_ratio());
                let idx = self.layout.counts_array_index(integer_value).min(active_counts.length() - 1);
                let recorded_value = self.layout.value_from_index(idx);
                let normalized_index = util::normalize_index(idx, active_counts.normalizing_index_offset(), active_counts.length());
                let c = active_counts.get_unchecked(normalized_index);
                c.fetch_add(1, Ordering::Relaxed);
                self.update_min_and_max(recorded_value);
                self.total_count.fetch_add(1, Ordering::Relaxed);
            }
            Ok(true)
        }
    }

    fn integer_to_double_value_conversion_ratio_locked(&self) -> f64 {
        unsafe { &*self.active_counts.load(Ordering::Acquire) }.integer_to_double_value_conversion_ratio()
    }

    fn highest_trackable_value_locked(&self) -> u64 {
        unsafe { &*self.active_counts.load(Ordering::Acquire) }
            .metadata()
            .highest_trackable_value
    }

    fn count_at_index_locked(&self, index: u32) -> u64 {
        unsafe { self.unsafe_get_count_at_index_locked(index) }
    }

    #[inline(never)]
    fn resize_and_record(&self, value: u64, idx: u32, count: u64) -> Result<(), RecordError> {
        unsafe {
            self.resize(value).map_err(RecordError::ResizeFailed)?;
            {
                let _csg = self.wrp.begin_writer_critical_section();
                let active_counts = &*self.active_counts.load(Ordering::Acquire);
                if idx >= active_counts.length() {
                    return Err(RecordError::ValueOutOfRangeResizeDisabled);
                }
                let normalized_index = util::normalize_index(idx, active_counts.normalizing_index_offset(), active_counts.length());
                active_counts.get_unchecked(normalized_index).fetch_add(count, Ordering::Relaxed);
            }

            self.update_min_and_max(value);
            self.total_count.fetch_add(count, Ordering::Relaxed);
            Ok(())
        }
    }

    fn to_integer_value(value: f64, ratio: f64) -> Result<u64, RecordError> {
        if !value.is_finite() || value < 0.0 {
            return Err(RecordError::ValueOutOfRangeResizeDisabled);
        }
        let scaled = value * ratio;
        if scaled > u64::MAX as f64 {
            return Err(RecordError::ValueOutOfRangeResizeDisabled);
        }
        Ok(scaled as u64)
    }

    #[inline(always)]
    fn to_integer_value_unchecked(value: f64, ratio: f64) -> u64 {
        (value * ratio) as u64
    }

    fn to_integer_value_saturating(value: f64, ratio: f64) -> u64 {
        if !value.is_finite() || value <= 0.0 {
            return 0;
        }
        let scaled = value * ratio;
        if scaled > u64::MAX as f64 {
            return u64::MAX;
        }
        scaled as u64
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
        concurrent_util::update_max_value(&self.layout, &self.raw_max_value, value);
    }

    #[inline]
    fn update_min_non_zero_value(&self, value: u64) {
        concurrent_util::update_min_non_zero_value(&self.layout, &self.raw_min_non_zero_value, value);
    }

    pub(crate) fn shift_values_left(&self, number_of_binary_orders_of_magnitude: u32) -> Result<(), ShiftError> {
        self.shift_values_left_with_conversion_ratio(
            number_of_binary_orders_of_magnitude,
            self.integer_to_double_value_conversion_ratio(),
        )
    }

    pub(crate) fn shift_values_left_with_conversion_ratio(
        &self,
        number_of_binary_orders_of_magnitude: u32,
        new_integer_to_double_value_conversion_ratio: f64,
    ) -> Result<(), ShiftError> {
        let mut mutation = self.begin_structural_mutation();
        mutation.shift_values_left_with_conversion_ratio(number_of_binary_orders_of_magnitude, new_integer_to_double_value_conversion_ratio)
    }

    fn shift_values_left_with_conversion_ratio_locked(
        &self,
        flip_guard: &PhaseFlipGuard<'_>,
        number_of_binary_orders_of_magnitude: u32,
        new_integer_to_double_value_conversion_ratio: f64,
    ) -> Result<(), ShiftError> {
        if number_of_binary_orders_of_magnitude == 0 {
            return Ok(());
        }
        let shift_amount = number_of_binary_orders_of_magnitude << self.layout.sub_bucket_half_count_magnitude;
        if self.get_total_count() == unsafe { self.unsafe_get_count_at_index_locked(0) } {
            return self.shift_normalizing_index_by_offset_locked(
                flip_guard,
                shift_amount as i32,
                false,
                new_integer_to_double_value_conversion_ratio,
            );
        }

        let active_counts = unsafe { &*self.active_counts.load(Ordering::Acquire) };
        let max_value_index = self.layout.counts_array_index(self.get_max_value());
        if max_value_index >= (active_counts.length() - shift_amount) {
            return Err(ShiftError::Overflow);
        }

        let max_before = self.raw_max_value.swap(ORIGINAL_MAX, Ordering::Relaxed);
        let min_before = self.raw_min_non_zero_value.swap(ORIGINAL_MIN, Ordering::Relaxed);
        let lowest_half_bucket_populated = min_before < ((self.layout.sub_bucket_half_count as u64) << self.layout.unit_magnitude);

        self.shift_normalizing_index_by_offset_locked(
            flip_guard,
            shift_amount as i32,
            lowest_half_bucket_populated,
            new_integer_to_double_value_conversion_ratio,
        )?;

        self.update_min_and_max(max_before << number_of_binary_orders_of_magnitude);
        if min_before != ORIGINAL_MIN {
            self.update_min_and_max(min_before << number_of_binary_orders_of_magnitude);
        }
        Ok(())
    }

    pub(crate) fn shift_values_right(&self, number_of_binary_orders_of_magnitude: u32) -> Result<(), ShiftError> {
        self.shift_values_right_with_conversion_ratio(
            number_of_binary_orders_of_magnitude,
            self.integer_to_double_value_conversion_ratio(),
        )
    }

    pub(crate) fn shift_values_right_with_conversion_ratio(
        &self,
        number_of_binary_orders_of_magnitude: u32,
        new_integer_to_double_value_conversion_ratio: f64,
    ) -> Result<(), ShiftError> {
        let mut mutation = self.begin_structural_mutation();
        mutation
            .shift_values_right_with_conversion_ratio(number_of_binary_orders_of_magnitude, new_integer_to_double_value_conversion_ratio)
    }

    fn shift_values_right_with_conversion_ratio_locked(
        &self,
        flip_guard: &PhaseFlipGuard<'_>,
        number_of_binary_orders_of_magnitude: u32,
        new_integer_to_double_value_conversion_ratio: f64,
    ) -> Result<(), ShiftError> {
        if number_of_binary_orders_of_magnitude == 0 {
            return Ok(());
        }
        let shift_amount = self.layout.sub_bucket_half_count * number_of_binary_orders_of_magnitude;
        if self.get_total_count() == unsafe { self.unsafe_get_count_at_index_locked(0) } {
            return self.shift_normalizing_index_by_offset_locked(
                flip_guard,
                -(shift_amount as i32),
                false,
                new_integer_to_double_value_conversion_ratio,
            );
        }

        let min_non_zero_value_index = self.layout.counts_array_index(self.get_min_non_zero_value());
        if min_non_zero_value_index < shift_amount + self.layout.sub_bucket_half_count {
            return Err(ShiftError::Underflow);
        }

        let max_before = self.raw_max_value.swap(ORIGINAL_MAX, Ordering::Relaxed);
        let min_before = self.raw_min_non_zero_value.swap(ORIGINAL_MIN, Ordering::Relaxed);

        self.shift_normalizing_index_by_offset_locked(
            flip_guard,
            -(shift_amount as i32),
            false,
            new_integer_to_double_value_conversion_ratio,
        )?;

        self.update_min_and_max(max_before >> number_of_binary_orders_of_magnitude);
        if min_before != ORIGINAL_MIN {
            self.update_min_and_max(min_before >> number_of_binary_orders_of_magnitude);
        }
        Ok(())
    }

    fn shift_normalizing_index_by_offset_locked(
        &self,
        flip_guard: &PhaseFlipGuard<'_>,
        offset_to_add: i32,
        lowest_half_bucket_populated: bool,
        new_integer_to_double_value_conversion_ratio: f64,
    ) -> Result<(), ShiftError> {
        let active_counts = unsafe { &*self.active_counts.load(Ordering::Acquire) };
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
            new_integer_to_double_value_conversion_ratio,
        )?;

        self.swap_active_inactive();
        flip_guard.flip();

        let inactive_counts = unsafe { &*self.inactive_counts.load(Ordering::Relaxed) };
        self.set_normalizing_index_offset_for_inactive(
            inactive_counts,
            new_offset,
            offset_to_add,
            lowest_half_bucket_populated,
            new_integer_to_double_value_conversion_ratio,
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
        new_integer_to_double_value_conversion_ratio: f64,
    ) -> Result<(), ShiftError> {
        let pre_shift_zero_index = util::normalize_index(0, inactive_counts.normalizing_index_offset(), inactive_counts.length());
        let zero_value_count = unsafe { inactive_counts.get_unchecked(pre_shift_zero_index).load(Ordering::Relaxed) };
        unsafe {
            inactive_counts.get_unchecked(pre_shift_zero_index).store(0, Ordering::Relaxed);
        }

        inactive_counts.set_normalizing_index_offset(new_normalizing_index_offset);

        if shifted_amount > 0 && lowest_half_bucket_populated {
            self.shift_lowest_inactive_half_bucket_contents_left(inactive_counts, shifted_amount as u32, pre_shift_zero_index);
        } else if lowest_half_bucket_populated && shifted_amount <= 0 {
            return Err(ShiftError::Underflow);
        }

        let new_zero_index = util::normalize_index(0, inactive_counts.normalizing_index_offset(), inactive_counts.length());
        unsafe {
            inactive_counts
                .get_unchecked(new_zero_index)
                .store(zero_value_count, Ordering::Relaxed);
        }
        inactive_counts.set_integer_to_double_value_conversion_ratio(new_integer_to_double_value_conversion_ratio);

        Ok(())
    }

    fn shift_lowest_inactive_half_bucket_contents_left(
        &self,
        inactive_counts: &InlineBackingArray<AtomicU64>,
        shift_amount: u32,
        pre_shift_zero_index: u32,
    ) {
        let number_of_binary_orders_of_magnitude = shift_amount >> self.layout.sub_bucket_half_count_magnitude;
        for from_index in 1..self.layout.sub_bucket_half_count {
            let to_value = self.layout.value_from_index(from_index) << number_of_binary_orders_of_magnitude;
            let to_index = self.layout.counts_array_index(to_value);
            let normalized_to_index = util::normalize_index(to_index, inactive_counts.normalizing_index_offset(), inactive_counts.length());
            let from_normalized_index = from_index + pre_shift_zero_index;
            let count_at_from_index = unsafe { inactive_counts.get_unchecked(from_normalized_index).load(Ordering::Relaxed) };
            unsafe {
                inactive_counts
                    .get_unchecked(normalized_to_index)
                    .store(count_at_from_index, Ordering::Relaxed);
                inactive_counts.get_unchecked(from_normalized_index).store(0, Ordering::Relaxed);
            }
        }
    }

    pub fn is_auto_resize(&self) -> bool {
        self.auto_resize.load(Ordering::Relaxed)
    }

    pub fn get_count_at_index(&self, index: u32) -> Option<u64> {
        let _g = self.wrp.reader_lock();
        let active_counts = unsafe { &*self.active_counts.load(Ordering::Acquire) };
        if index >= active_counts.length() {
            return None;
        }

        Some(unsafe { self.unsafe_get_count_at_index_locked(index) })
    }

    pub(crate) fn unsafe_get_count_at_index(&self, index: u32) -> u64 {
        let _g = self.wrp.reader_lock();
        unsafe { self.unsafe_get_count_at_index_locked(index) }
    }

    unsafe fn unsafe_get_count_at_index_locked(&self, index: u32) -> u64 {
        unsafe {
            let active_counts = &*self.active_counts.load(Ordering::Acquire);
            let inactive_counts = &*self.inactive_counts.load(Ordering::Relaxed);
            debug_assert_eq!(active_counts.length(), inactive_counts.length());
            let active_index = util::normalize_index(index, active_counts.normalizing_index_offset(), active_counts.length());
            let inactive_index = util::normalize_index(index, inactive_counts.normalizing_index_offset(), inactive_counts.length());
            let active_count = active_counts.get_unchecked(active_index);
            let inactive_count = inactive_counts.get_unchecked(inactive_index);
            active_count.load(Ordering::Relaxed) + inactive_count.load(Ordering::Relaxed)
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

    pub(crate) fn set_integer_to_double_value_conversion_ratio(&self, ratio: f64) {
        let mut mutation = self.begin_structural_mutation();
        mutation.set_integer_to_double_value_conversion_ratio(ratio);
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

    unsafe fn copy_inactive_counts_contents_on_resize(
        &self,
        old_inactive: &InlineBackingArray<AtomicU64>,
        new_inactive: &InlineBackingArray<AtomicU64>,
        _counts_delta: u32,
    ) {
        let old_offset = old_inactive.normalizing_index_offset();
        new_inactive.set_normalizing_index_offset(old_offset);
        new_inactive.set_integer_to_double_value_conversion_ratio(old_inactive.integer_to_double_value_conversion_ratio());

        for logical_index in 0..old_inactive.length() {
            let old_index = util::normalize_index(logical_index, old_offset, old_inactive.length());
            let new_index = util::normalize_index(logical_index, old_offset, new_inactive.length());
            let value = old_inactive.get_unchecked(old_index).load(Ordering::Relaxed);
            new_inactive.get_unchecked(new_index).store(value, Ordering::Relaxed);
        }
    }

    fn swap_active_inactive(&self) {
        let active_ptr = self.active_counts.load(Ordering::Relaxed);
        let inactive_ptr = self.inactive_counts.load(Ordering::Relaxed);
        // Publish the prepared inactive array, including its offset and double
        // conversion ratio metadata. Recorders load active_counts with Acquire
        // before using those fields for slot selection.
        self.active_counts.store(inactive_ptr, Ordering::SeqCst);
        self.inactive_counts.store(active_ptr, Ordering::SeqCst);
    }

    #[inline(never)]
    pub fn resize(&self, new_highest_trackable_value: u64) -> Result<(), CreationError> {
        let mut mutation = self.begin_structural_mutation();
        mutation.resize(new_highest_trackable_value)
    }

    fn resize_locked(&self, flip_guard: &PhaseFlipGuard<'_>, new_highest_trackable_value: u64) -> Result<(), CreationError> {
        unsafe {
            let active_counts = &*self.active_counts.load(Ordering::Acquire);
            let inactive_counts = &*self.inactive_counts.load(Ordering::Relaxed);

            assert!(active_counts.length() == inactive_counts.length());

            let metadata = self.layout.resized_metadata_for_value(new_highest_trackable_value)?;
            let new_array_length = metadata.counts_array_length;

            if new_array_length <= active_counts.length() {
                return Ok(());
            }
            let counts_delta = new_array_length - active_counts.length();

            let new_inactive_counts_1 = InlineBackingArray::<AtomicU64>::new(metadata);
            let new_inactive_counts_2 = InlineBackingArray::<AtomicU64>::new(metadata);

            let previous_inactive_counts = self.inactive_counts.load(Ordering::Relaxed);
            self.copy_inactive_counts_contents_on_resize(&*previous_inactive_counts, &*new_inactive_counts_1, counts_delta);
            self.inactive_counts.store(new_inactive_counts_1, Ordering::SeqCst);

            self.swap_active_inactive();
            flip_guard.flip();

            let previous_active_counts = self.inactive_counts.load(Ordering::Relaxed);
            self.copy_inactive_counts_contents_on_resize(&*previous_active_counts, &*new_inactive_counts_2, counts_delta);
            self.inactive_counts.store(new_inactive_counts_2, Ordering::SeqCst);

            self.swap_active_inactive();
            flip_guard.flip();

            let guard = epoch::pin();
            Self::retire_counts_array(previous_active_counts, &guard);
            Self::retire_counts_array(previous_inactive_counts, &guard);
            guard.flush();

            assert!(new_array_length == (*self.active_counts.load(Ordering::Acquire)).length());
            assert!(new_array_length == (*self.inactive_counts.load(Ordering::Relaxed)).length());

            Ok(())
        }
    }

    pub fn set_auto_resize(&self, resize: bool) {
        self.auto_resize.store(resize, Ordering::Relaxed);
    }

    pub(crate) unsafe fn clear_counts(&self) {
        let mut mutation = self.begin_structural_mutation();
        mutation.flip();
        mutation.clear_counts();
    }

    unsafe fn clear_counts_locked(&self) {
        let active_counts = self.active_counts.load(Ordering::Acquire);
        let inactive_counts = self.inactive_counts.load(Ordering::Relaxed);
        for ix in 0..(*active_counts).length() {
            (*active_counts).get_unchecked(ix).store(0, Ordering::Relaxed);
            (*inactive_counts).get_unchecked(ix).store(0, Ordering::Relaxed);
        }
        (*active_counts).set_normalizing_index_offset(0);
        (*inactive_counts).set_normalizing_index_offset(0);
        self.total_count.store(0, Ordering::Relaxed);
        self.raw_max_value
            .store(ORIGINAL_MAX | self.layout.unit_magnitude_mask, Ordering::Relaxed);
        self.raw_min_non_zero_value.store(ORIGINAL_MIN, Ordering::Relaxed);
        let meta_data = &self.meta_data as *const HistogramMetaData as *mut HistogramMetaData;
        (*meta_data).clear();
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
                check_eq!(self.unsafe_get_count_at_index(i), other.unsafe_get_count_at_index(i));
            }
        } else {
            let other_last = other_len - 1;
            let mut iterator = RecordedValuesIterator::from_readable(self.read_view());
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

    pub(in crate::concurrent) fn write_inactive_to_active(&self) {
        unsafe {
            let _lg = self.wrp.reader_lock();
            let active_counts = self.active_counts.load(Ordering::Acquire);
            let inactive_counts = self.inactive_counts.load(Ordering::Relaxed);
            for idx in 0..(*active_counts).length() {
                let active_index = util::normalize_index(idx, (*active_counts).normalizing_index_offset(), (*active_counts).length());
                let inactive_index = util::normalize_index(idx, (*inactive_counts).normalizing_index_offset(), (*inactive_counts).length());
                let inactive_loc = (*inactive_counts).get_unchecked(inactive_index);
                let count = inactive_loc.load(Ordering::Relaxed);
                (*active_counts).get_unchecked(active_index).fetch_add(count, Ordering::Relaxed);
                inactive_loc.store(0, Ordering::Relaxed);
            }
        }
    }

    // Use only when callers can guarantee no concurrent structural mutation.
    pub(crate) unsafe fn unsafe_as_snapshot(&self) -> Snapshot<'_, Self> {
        Snapshot::new(self)
    }

    pub fn as_snapshot(&mut self) -> ResizableSnapshot<'_> {
        ResizableSnapshot::new(Snapshot::new(self))
    }
}

impl ResizableStructuralMutation<'_> {
    pub(crate) fn flip(&mut self) {
        self.guard.flip();
    }

    pub(crate) fn integer_to_double_value_conversion_ratio(&self) -> f64 {
        self.histogram.integer_to_double_value_conversion_ratio_locked()
    }

    pub(crate) fn highest_trackable_value(&self) -> u64 {
        self.histogram.highest_trackable_value_locked()
    }

    pub(crate) fn count_at_index(&self, index: u32) -> u64 {
        self.histogram.count_at_index_locked(index)
    }

    pub(crate) fn shift_values_left_with_conversion_ratio(
        &mut self,
        number_of_binary_orders_of_magnitude: u32,
        new_integer_to_double_value_conversion_ratio: f64,
    ) -> Result<(), ShiftError> {
        self.histogram.shift_values_left_with_conversion_ratio_locked(
            &self.guard,
            number_of_binary_orders_of_magnitude,
            new_integer_to_double_value_conversion_ratio,
        )
    }

    pub(crate) fn shift_values_right_with_conversion_ratio(
        &mut self,
        number_of_binary_orders_of_magnitude: u32,
        new_integer_to_double_value_conversion_ratio: f64,
    ) -> Result<(), ShiftError> {
        self.histogram.shift_values_right_with_conversion_ratio_locked(
            &self.guard,
            number_of_binary_orders_of_magnitude,
            new_integer_to_double_value_conversion_ratio,
        )
    }

    pub(crate) fn set_integer_to_double_value_conversion_ratio(&mut self, ratio: f64) {
        unsafe {
            let inactive_counts = &*self.histogram.inactive_counts.load(Ordering::Relaxed);
            inactive_counts.set_integer_to_double_value_conversion_ratio(ratio);

            self.histogram.swap_active_inactive();
            self.guard.flip();

            let inactive_counts = &*self.histogram.inactive_counts.load(Ordering::Relaxed);
            inactive_counts.set_integer_to_double_value_conversion_ratio(ratio);

            self.histogram.swap_active_inactive();
            self.guard.flip();
        }
    }

    pub(crate) fn resize(&mut self, new_highest_trackable_value: u64) -> Result<(), CreationError> {
        self.histogram.resize_locked(&self.guard, new_highest_trackable_value)
    }

    pub(crate) unsafe fn clear_counts(&mut self) {
        self.histogram.clear_counts_locked();
    }
}

impl ResizableConcurrentReadView<'_> {
    pub fn percentiles(&self, percentile_ticks_per_half_distance: u32) -> PercentileIterator<&'_ Self> {
        PercentileIterator::from_readable(self, percentile_ticks_per_half_distance)
    }

    pub fn linear_bucket_values(&self, value_units_per_bucket: u64) -> LinearIterator<&'_ Self> {
        LinearIterator::from_readable(self, value_units_per_bucket)
    }

    pub fn logarithmic_bucket_values(&self, value_units_in_first_bucket: u64, log_base: f64) -> LogarithmicIterator<&'_ Self> {
        LogarithmicIterator::from_readable(self, value_units_in_first_bucket, log_base)
    }

    pub fn all_values(&self) -> AllValuesIterator<&'_ Self> {
        AllValuesIterator::from_readable(self)
    }

    pub fn recorded_values(&self) -> RecordedValuesIterator<&'_ Self> {
        RecordedValuesIterator::from_readable(self)
    }

    pub(crate) fn get_min_non_zero_value(&self) -> u64 {
        self.min_non_zero_value
    }
}

impl ReadableHistogram for ResizableConcurrentReadView<'_> {
    fn settings(&self) -> HistogramSettings {
        self.settings.clone()
    }

    fn array_length(&self) -> u32 {
        self.settings.counts_array_length
    }

    fn get_total_count(&self) -> u64 {
        self.total_count
    }

    fn current_total_count(&self) -> u64 {
        self.histogram.total_count.load(Ordering::Relaxed)
    }

    fn unsafe_get_count_at_index(&self, index: u32) -> u64 {
        unsafe {
            let active_counts = &*self.active_counts;
            let inactive_counts = &*self.inactive_counts;
            let active_index = util::normalize_index(index, self.active_normalizing_index_offset, active_counts.length());
            let inactive_index = util::normalize_index(index, self.inactive_normalizing_index_offset, inactive_counts.length());
            let active_count = active_counts.get_unchecked(active_index);
            let inactive_count = inactive_counts.get_unchecked(inactive_index);
            active_count.load(Ordering::Relaxed) + inactive_count.load(Ordering::Relaxed)
        }
    }

    fn get_max_value(&self) -> u64 {
        self.max_value
    }

    fn meta_data(&self) -> &HistogramMetaData {
        &self.histogram.meta_data
    }

    fn integer_to_double_value_conversion_ratio(&self) -> f64 {
        self.integer_to_double_value_conversion_ratio
    }

    fn normalizing_index_offset(&self) -> i32 {
        self.active_normalizing_index_offset
    }
}

impl EncodableHistogram for ResizableConcurrentReadView<'_> {}

impl ConstructableHistogram for ResizableConcurrentHistogram {
    fn new(lowest_discernible_value: u64, highest_trackable_value: u64, significant_value_digits: u8) -> Result<Self, CreationError> {
        ResizableConcurrentHistogram::with_low_high_sigvdig(lowest_discernible_value, highest_trackable_value, significant_value_digits)
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

impl RecordableHistogram for ResizableConcurrentHistogram {
    fn fresh(settings: &HistogramSettings) -> Result<ResizableConcurrentHistogram, CreationError> {
        let lowest_discernable = settings.lowest_discernible_value;
        let highest_trackable = settings.highest_trackable_value;
        let sigvdig = settings.number_of_significant_value_digits as u8;
        ResizableConcurrentHistogram::with_low_high_sigvdig(lowest_discernable, highest_trackable, sigvdig)
    }
    #[inline(always)]
    fn meta_data_mut(&mut self) -> &mut HistogramMetaData {
        &mut self.meta_data
    }
    #[inline(always)]
    unsafe fn clear_counts(&self) {
        ResizableConcurrentHistogram::clear_counts(self);
    }
    fn equals(&self, other: &Self) -> bool {
        ResizableConcurrentHistogram::equals(self, other)
    }
    fn get_min_non_zero_value(&self) -> u64 {
        ResizableConcurrentHistogram::get_min_non_zero_value(self)
    }
    #[inline(always)]
    fn record_value(&self, value: u64) -> Result<(), RecordError> {
        ResizableConcurrentHistogram::record_value(self, value)
    }
    #[inline(always)]
    fn record_value_with_count(&self, value: u64, count: u64) -> Result<(), RecordError> {
        ResizableConcurrentHistogram::record_value_with_count(self, value, count)
    }
}

impl ReadableHistogram for ResizableConcurrentHistogram {
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
        self.get_max_value()
    }
    fn meta_data(&self) -> &HistogramMetaData {
        &self.meta_data
    }
    fn integer_to_double_value_conversion_ratio(&self) -> f64 {
        ResizableConcurrentHistogram::integer_to_double_value_conversion_ratio(self)
    }

    fn normalizing_index_offset(&self) -> i32 {
        ResizableConcurrentHistogram::normalizing_index_offset(self)
    }
}
