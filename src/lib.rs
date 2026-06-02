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
//! let mut histogram = Histogram::builder()
//!     .significant_digits(3)
//!     .highest_trackable_value(60_000)
//!     .build()
//!     .unwrap();
//! histogram.record_value(42).unwrap();
//! assert_eq!(histogram.get_total_count(), 1);
//! ```
//!
//! For concurrent recording, prefer the recorder constructors in
//! [`concurrent::recorder`]. Writers record without taking the reader lock, while
//! a sampling thread periodically calls
//! [`locking_sample`](concurrent::ResizableRecorder::locking_sample) and
//! [`resample`](concurrent::ResizableLockingSample::resample).
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
    ConcurrentDoubleHistogram, ConcurrentDoubleHistogramBuilder, ConcurrentDoubleHistogramWithPolicy, ConcurrentDoubleReadView,
    ConcurrentDoubleSnapshot, DoubleLockingSample, DoubleRecorder, FixedConcurrentHistogram, FixedConcurrentHistogramBuilder,
    FixedLockingSample, FixedRecorder, FixedSnapshot, ResizableConcurrentHistogram, ResizableConcurrentHistogramBuilder,
    ResizableConcurrentReadView, ResizableLockingSample, ResizableRecorder, ResizableSnapshot, SaturatingConcurrentDoubleHistogram,
    SaturatingDoubleRecorder, SaturatingSingleWriterDoubleRecorder, SingleWriterDoubleLockingSample, SingleWriterDoubleRecorder,
    SingleWriterLockingSample, SingleWriterRecorder,
};
pub use crate::core::{
    Counter, CreationError, DoubleCreationError, EncodableHistogram, HistogramMetaData, HistogramSettings, IterableHistogram,
    OverflowPolicy, RecordError, SaturateOnOverflow, ShiftError, SubtractionError, ThrowOnOverflow,
};
pub use crate::iteration::IterationError;
pub use crate::st::{
    DoubleHistogram, DoubleHistogramBuilder, DoubleHistogramWithPolicy, Histogram, HistogramBuilder, HistogramWithCounter,
    SaturatingDoubleHistogram,
};

#[cfg(test)]
pub mod tests;
