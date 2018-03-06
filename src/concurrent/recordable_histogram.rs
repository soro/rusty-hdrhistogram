use core::*;

pub trait RecordableHistogram
    : ReadableHistogram + MutSliceableHistogram<u64> + Sized {
    fn fresh(settings: &HistogramSettings) -> Result<Self, CreationError>;
    fn meta_data_mut(&mut self) -> &mut HistogramMetaData;
    unsafe fn clear_counts(&self);
    fn equals(&mut self, other: &mut Self) -> bool;
    fn record_value(&self, value: u64) -> Result<(), RecordError>;
    fn record_value_with_count(&self, value: u64, count: u64) -> Result<(), RecordError>;
}
