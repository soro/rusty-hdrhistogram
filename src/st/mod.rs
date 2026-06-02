//! Single-threaded histogram types.
//!
//! Use [`Histogram`] for integer values and [`DoubleHistogram`] for floating
//! point values. [`HistogramWithCounter`] exposes the same integer histogram
//! with explicit counter storage. The `SaturatingDoubleHistogram` alias clamps
//! slot selection for finite values that exceed the current representable range.

pub(crate) mod backing_array;
mod double_histogram;
mod histogram;

pub use self::double_histogram::{DoubleHistogram, DoubleHistogramBuilder, DoubleHistogramWithPolicy, SaturatingDoubleHistogram};
pub use self::histogram::{Histogram, HistogramBuilder, HistogramWithCounter};
