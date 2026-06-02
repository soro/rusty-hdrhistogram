use crate::concurrent::recordable_histogram::RecordableHistogram;
use crate::concurrent::resizable_histogram::ResizableConcurrentHistogram;
use crate::concurrent::static_histogram::StaticHistogram;
use crate::core::{EncodableHistogram, HistogramMetaData, HistogramSettings, IterableHistogram, ReadableHistogram};
use crate::iteration::*;

pub(crate) struct Snapshot<'a, T: 'a + RecordableHistogram>(&'a T);

impl<'a, T: RecordableHistogram> Snapshot<'a, T> {
    pub(in crate::concurrent) fn new(histogram: &T) -> Snapshot<'_, T> {
        Snapshot(histogram)
    }

    pub(crate) fn percentiles(&self, percentile_ticks_per_half_distance: u32) -> PercentileIterator<&'_ Self> {
        PercentileIterator::new(self, percentile_ticks_per_half_distance)
    }

    pub(crate) fn linear_bucket_values(&self, value_units_per_bucket: u64) -> LinearIterator<&'_ Self> {
        LinearIterator::new(self, value_units_per_bucket)
    }

    pub(crate) fn logarithmic_bucket_values(&self, value_units_in_first_bucket: u64, log_base: f64) -> LogarithmicIterator<&'_ Self> {
        LogarithmicIterator::new(self, value_units_in_first_bucket, log_base)
    }

    pub(crate) fn all_values(&self) -> AllValuesIterator<&'_ Self> {
        AllValuesIterator::new(self)
    }

    pub(crate) fn recorded_values(&self) -> RecordedValuesIterator<&'_ Self> {
        RecordedValuesIterator::new(self)
    }

    pub(crate) fn equals(&self, other: &Snapshot<'_, T>) -> bool {
        self.0.equals(other.0)
    }

    pub(crate) fn counts_array_length(&self) -> u32 {
        self.0.array_length()
    }

    pub(crate) fn get_count_at_index(&self, index: u32) -> Option<u64> {
        if index >= self.0.array_length() {
            return None;
        }
        Some(self.0.unsafe_get_count_at_index(index))
    }

    pub(crate) fn get_count_at_value(&self, value: u64) -> Option<u64> {
        let settings = self.0.settings();
        let index = settings.counts_array_index(value);
        if index >= settings.counts_array_length {
            return None;
        }
        Some(self.0.unsafe_get_count_at_index(index))
    }

    pub(crate) fn get_highest_trackable_value(&self) -> u64 {
        self.0.settings().highest_trackable_value
    }

    pub(crate) fn get_min_non_zero_value(&self) -> u64 {
        self.0.get_min_non_zero_value()
    }
}

impl<'a, T: RecordableHistogram> ReadableHistogram for Snapshot<'a, T> {
    fn settings(&self) -> HistogramSettings {
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
    fn meta_data(&self) -> &HistogramMetaData {
        self.0.meta_data()
    }
    fn integer_to_double_value_conversion_ratio(&self) -> f64 {
        self.0.integer_to_double_value_conversion_ratio()
    }

    fn normalizing_index_offset(&self) -> i32 {
        self.0.normalizing_index_offset()
    }
}

impl<'a, T: RecordableHistogram> IterableHistogram for Snapshot<'a, T> {}

pub struct StaticSnapshot<'a>(Snapshot<'a, StaticHistogram>);

pub struct ResizableSnapshot<'a>(Snapshot<'a, ResizableConcurrentHistogram>);

macro_rules! impl_snapshot_wrapper {
    ($snapshot:ident, $histogram:ty) => {
        impl<'a> $snapshot<'a> {
            pub(in crate::concurrent) fn new(snapshot: Snapshot<'a, $histogram>) -> Self {
                $snapshot(snapshot)
            }

            pub fn percentiles(&self, percentile_ticks_per_half_distance: u32) -> PercentileIterator<&'_ Self> {
                PercentileIterator::new(self, percentile_ticks_per_half_distance)
            }

            pub fn linear_bucket_values(&self, value_units_per_bucket: u64) -> LinearIterator<&'_ Self> {
                LinearIterator::new(self, value_units_per_bucket)
            }

            pub fn logarithmic_bucket_values(&self, value_units_in_first_bucket: u64, log_base: f64) -> LogarithmicIterator<&'_ Self> {
                LogarithmicIterator::new(self, value_units_in_first_bucket, log_base)
            }

            pub fn all_values(&self) -> AllValuesIterator<&'_ Self> {
                AllValuesIterator::new(self)
            }

            pub fn recorded_values(&self) -> RecordedValuesIterator<&'_ Self> {
                RecordedValuesIterator::new(self)
            }

            pub fn equals(&self, other: &Self) -> bool {
                self.0.equals(&other.0)
            }

            pub fn settings(&self) -> HistogramSettings {
                self.0.settings()
            }

            pub fn counts_array_length(&self) -> u32 {
                self.0.counts_array_length()
            }

            pub fn get_count_at_index(&self, index: u32) -> Option<u64> {
                self.0.get_count_at_index(index)
            }

            pub fn get_count_at_value(&self, value: u64) -> Option<u64> {
                self.0.get_count_at_value(value)
            }

            pub fn get_total_count(&self) -> u64 {
                self.0.get_total_count()
            }

            pub fn get_max_value(&self) -> u64 {
                self.0.get_max_value()
            }

            pub fn get_min_non_zero_value(&self) -> u64 {
                self.0.get_min_non_zero_value()
            }

            pub fn get_highest_trackable_value(&self) -> u64 {
                self.0.get_highest_trackable_value()
            }

            pub fn meta_data(&self) -> &HistogramMetaData {
                self.0.meta_data()
            }
        }

        impl<'a> ReadableHistogram for $snapshot<'a> {
            fn settings(&self) -> HistogramSettings {
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

            fn meta_data(&self) -> &HistogramMetaData {
                self.0.meta_data()
            }

            fn integer_to_double_value_conversion_ratio(&self) -> f64 {
                self.0.integer_to_double_value_conversion_ratio()
            }

            fn normalizing_index_offset(&self) -> i32 {
                self.0.normalizing_index_offset()
            }
        }

        impl<'a> IterableHistogram for $snapshot<'a> {}

        impl<'a> EncodableHistogram for $snapshot<'a> {}
    };
}

impl_snapshot_wrapper!(StaticSnapshot, StaticHistogram);
impl_snapshot_wrapper!(ResizableSnapshot, ResizableConcurrentHistogram);
