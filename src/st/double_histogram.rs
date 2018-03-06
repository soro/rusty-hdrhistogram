use core::*;
use core::meta_data::HistogramMetaData;
use st::Histogram;

pub struct DoubleHistogram {
    pub meta_data: HistogramMetaData,
    settings: HistogramSettings,
    inner: Histogram<u64>,
}

macro_rules! scale {
    ($slf:ident, $e:ident) => {
        $slf.inner.$e() as f64 * $slf.settings.integer_to_double_value_conversion_ratio
    };
}
