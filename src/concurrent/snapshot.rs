use concurrent::{StaticHistogram, ResizableHistogram};
use concurrent::recordable_histogram::RecordableHistogram;
use core::{HistogramMetaData, HistogramSettings, MutSliceableHistogram, ReadSliceableHistogram, ReadableHistogram};
use iteration::*;
use std::{mem, slice};
use std::ops::Deref;
use std::sync::atomic::Ordering;

pub struct Snapshot<'a, T: 'a + RecordableHistogram>(&'a mut T);

impl<'a, T: RecordableHistogram> Deref for Snapshot<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.0 as &T
    }
}

impl<'a, T: RecordableHistogram> Snapshot<'a, T> {
    pub unsafe fn new(histogram: &mut T) -> Snapshot<T> {
        Snapshot(histogram)
    }

    pub fn percentiles(&self, percentile_ticks_per_half_distance: u32) -> PercentileIterator<T> {
        PercentileIterator::new(self.0, percentile_ticks_per_half_distance)
    }

    pub fn linear_bucket_values(&self, value_units_per_bucket: u64) -> LinearIterator<T> {
        LinearIterator::new(self.0, value_units_per_bucket)
    }

    pub fn logarithmic_bucket_values(&self, value_units_in_first_bucket: u64, log_base: f64) -> LogarithmicIterator<T> {
        LogarithmicIterator::new(self.0, value_units_in_first_bucket, log_base)
    }

    pub fn all_values(&self) -> AllValuesIterator<T> {
        AllValuesIterator::new(self.0)
    }

    pub fn recorded_values(&self) -> RecordedValuesIterator<T> {
        RecordedValuesIterator::new(self.0)
    }
}

impl<'a> ReadSliceableHistogram<u64> for Snapshot<'a, StaticHistogram> {
    fn get_counts_slice<'b>(&'b self, length: u32) -> Option<&'b [u64]> {
        unsafe {
            let counts = self.counts.load(Ordering::Relaxed);
            if length <= (*counts).length() {
                return Some(slice::from_raw_parts(
                    mem::transmute((*counts).get_array_ptr()),
                    length as usize,
                ));
            }
            None
        }
    }
}

impl<'a> ReadSliceableHistogram<u64> for Snapshot<'a, ResizableHistogram> {
    fn get_counts_slice<'b>(&'b self, length: u32) -> Option<&'b [u64]> { unsafe {
        if length > self.0.settings().counts_array_length { return None; }
        else {
            if self.0.get_total_count() != 0 {
                self.0.write_inactive_to_active();
            }
            return Some(slice::from_raw_parts(
                mem::transmute((*self.0.active_counts.load(Ordering::Relaxed)).get_array_ptr()),
                length as usize,
            ));
        }
    }}
}

impl<'a, T: RecordableHistogram> MutSliceableHistogram<u64> for Snapshot<'a, T> {
    fn get_counts_slice_mut<'b>(&'b mut self, length: u32) -> Option<&'b mut [u64]> {
        self.0.get_counts_slice_mut(length)
    }
}

impl<'a, T: RecordableHistogram> ReadableHistogram for Snapshot<'a, T> {
    fn settings(&self) -> &HistogramSettings {
        self.0.settings()
    }
    fn array_length(&self) -> u32 {
        self.0.array_length()
    }
    fn get_total_count(&self) -> u64 {
        self.0.get_total_count()
    }
    fn unsafe_get_count_at_index(&self, idx: u32) -> u64 {
        self.0.unsafe_get_count_at_index(idx)
    }
    fn get_max_value(&self) -> u64 {
        self.0.get_max_value()
    }
    fn meta_data(&self) -> &HistogramMetaData { self.0.meta_data() }
}

impl<'a, T: RecordableHistogram> PartialEq for Snapshot<'a, T> {
    fn eq(&self, other: &Self) -> bool {
        // this is safe because we really only read from the values and use the &mut requirement to enforce exclusive access
        #[allow(mutable_transmutes)]
        unsafe { mem::transmute::<&T, &mut T>(self.0).equals(mem::transmute::<&T, &mut T>(other.0)) }
    }
}
