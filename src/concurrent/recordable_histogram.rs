use crate::core::*;

mod sealed {
    pub trait Sealed {}
}

impl sealed::Sealed for super::ResizableConcurrentHistogram {}
impl sealed::Sealed for super::FixedConcurrentHistogram {}

pub(crate) trait RecordableHistogram: sealed::Sealed + ReadableHistogram + Sized {
    fn fresh(settings: &HistogramSettings) -> Result<Self, CreationError>;
    fn meta_data_mut(&mut self) -> &mut HistogramMetaData;
    unsafe fn clear_counts(&self);
    fn equals(&self, other: &Self) -> bool;
    fn get_min_non_zero_value(&self) -> u64;
    fn record_value(&self, value: u64) -> Result<(), RecordError>;
    fn record_value_with_count(&self, value: u64, count: u64) -> Result<(), RecordError>;
}
