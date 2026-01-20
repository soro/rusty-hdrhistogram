use crate::concurrent::recordable_histogram::RecordableHistogram;
use crate::core::{HistogramMetaData, HistogramSettings, ReadableHistogram};
use crate::iteration::*;
use std::ops::Deref;

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

    pub fn equals(&mut self, other: &mut Snapshot<'_, T>) -> bool {
        let this = &mut *self.0;
        let other = &mut *other.0;
        this.equals(other)
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
