use core::{HistogramMetaData, HistogramSettings};

pub trait ReadableHistogram {
    // required for iteration
    fn settings(&self) -> &HistogramSettings;
    fn array_length(&self) -> u32;
    fn get_total_count(&self) -> u64;
    fn unsafe_get_count_at_index(&self, idx: u32) -> u64;

    // required for serialization
    fn get_max_value(&self) -> u64;

    fn meta_data(&self) -> &HistogramMetaData;
}
