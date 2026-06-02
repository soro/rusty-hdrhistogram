//! Single-threaded histogram types.
//!
//! Use [`Histogram`] for integer values and [`DoubleHistogram`] for floating
//! point values. The `SaturatingDoubleHistogram` alias clamps slot selection for
//! finite values that exceed the current representable range.

pub(crate) mod backing_array;
mod double_histogram;
mod histogram;

pub use self::double_histogram::{DoubleHistogram, DoubleHistogramImpl, SaturatingDoubleHistogram};
pub use self::histogram::Histogram;
