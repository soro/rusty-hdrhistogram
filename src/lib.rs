#![allow(dead_code)]
#![recursion_limit = "128"]

//! Rust port of HdrHistogram with integer, double, concurrent recorder, and
//! Java-compatible encoding/logging support.
//!
//! The most common single-threaded entry point is [`Histogram`]:
//!
//! ```
//! use hdrhistogram::Histogram;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut histogram = Histogram::builder()
//!     .significant_digits(3)
//!     .highest_trackable_value(60_000)
//!     .build()?;
//! histogram.record_value(42)?;
//! assert_eq!(histogram.get_total_count(), 1);
//! # Ok(())
//! # }
//! ```
//!
//! For concurrent recording, prefer the recorder types in [`concurrent`].
//! Writers keep recording while a sampling thread
//! periodically calls
//! [`begin_interval_sample`](concurrent::ResizableRecorder::begin_interval_sample) and
//! [`resample`](concurrent::ResizableIntervalSample::resample).
//!
//! Java-compatible V2 encoding, compressed/Base64 helpers, histogram log
//! reader/writer building blocks, and report generation live in [`encoding`].
//! Compression requires the `encoding-compression` feature; Base64 log helpers
//! and report generation require `encoding-base64`.

#[macro_use]
mod core;
pub mod concurrent;
pub mod encoding;
pub mod iteration;
pub mod st;

pub use crate::concurrent::{
    DoubleRecorder, FixedRecorder, ResizableRecorder, SaturatingDoubleRecorder, SaturatingSingleWriterDoubleRecorder,
    SingleWriterDoubleRecorder, SingleWriterRecorder,
};
pub use crate::core::{
    CreationError, DoubleCreationError, HistogramMetaData, HistogramSettings, RecordError, ShiftError, SubtractionError,
};
pub use crate::iteration::IterationError;
pub use crate::st::{DoubleHistogram, Histogram, SaturatingDoubleHistogram};

#[cfg(test)]
pub mod tests;
