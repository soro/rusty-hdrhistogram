pub mod histogram;
pub mod double_histogram;
pub mod backing_array;

pub use self::double_histogram::{DoubleHistogram, DoubleHistogramImpl, SaturatingDoubleHistogram};
pub use self::histogram::Histogram;
