use crate::core::{HistogramMetaData, HistogramSettings};

pub(crate) mod sealed {
    pub trait Sealed {}

    impl<T: super::ReadableHistogram + ?Sized> Sealed for &T {}
}

#[doc(hidden)]
pub trait ReadableHistogram: sealed::Sealed {
    // required for iteration
    fn settings(&self) -> HistogramSettings;
    fn array_length(&self) -> u32;
    fn get_total_count(&self) -> u64;
    fn unsafe_get_count_at_index(&self, idx: u32) -> u64;

    fn current_total_count(&self) -> u64 {
        self.get_total_count()
    }

    fn get_max_value(&self) -> u64;

    fn meta_data(&self) -> &HistogramMetaData;

    fn integer_to_double_value_conversion_ratio(&self) -> f64 {
        1.0
    }

    fn normalizing_index_offset(&self) -> i32 {
        0
    }
}

/// Histogram data that can be exposed through public scan-style iterators.
///
/// Live concurrent histograms intentionally do not implement this trait; obtain
/// a recorder sample/snapshot before iterating them.
pub trait IterableHistogram: ReadableHistogram {}

/// Histogram data that can be encoded without mixing structural epochs.
///
/// Live concurrent histograms intentionally do not implement this trait; use a
/// captured read view or a recorder sample/snapshot.
///
/// Captured concurrent read views are structurally stable, but they may still
/// expose live count cells. Use recorder samples/snapshots when the encoded
/// histogram must represent a frozen count set.
pub trait EncodableHistogram: ReadableHistogram {}

impl<T: ReadableHistogram + ?Sized> ReadableHistogram for &T {
    fn settings(&self) -> HistogramSettings {
        (**self).settings()
    }

    fn array_length(&self) -> u32 {
        (**self).array_length()
    }

    fn get_total_count(&self) -> u64 {
        (**self).get_total_count()
    }

    fn unsafe_get_count_at_index(&self, idx: u32) -> u64 {
        (**self).unsafe_get_count_at_index(idx)
    }

    fn current_total_count(&self) -> u64 {
        (**self).current_total_count()
    }

    fn get_max_value(&self) -> u64 {
        (**self).get_max_value()
    }

    fn meta_data(&self) -> &HistogramMetaData {
        (**self).meta_data()
    }

    fn integer_to_double_value_conversion_ratio(&self) -> f64 {
        (**self).integer_to_double_value_conversion_ratio()
    }

    fn normalizing_index_offset(&self) -> i32 {
        (**self).normalizing_index_offset()
    }
}

impl<T: IterableHistogram + ?Sized> IterableHistogram for &T {}

impl<T: EncodableHistogram + ?Sized> EncodableHistogram for &T {}
