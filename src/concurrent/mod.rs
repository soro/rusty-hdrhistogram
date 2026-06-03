//! Concurrent histograms and recorder types.
//!
//! Most users should use recorder types such as [`ResizableRecorder`] instead of
//! recording directly into concurrent histograms. Recorders keep writer-side
//! updates lock-free with respect to sampling and expose snapshots through
//! [`ResizableIntervalSample`] or [`FixedIntervalSample`].

pub(crate) mod concurrent_util;
mod double_histogram;
pub(crate) mod inline_backing_array;
mod interval_sample;
pub(crate) mod recordable_histogram;
mod recorder;
mod resizable_histogram;
mod snapshot;
mod static_histogram;
pub(crate) mod writer_reader_phaser;

pub use self::double_histogram::{
    ConcurrentDoubleHistogram, ConcurrentDoubleHistogramBuilder, ConcurrentDoubleHistogramWithPolicy, ConcurrentDoubleReadView,
    ConcurrentDoubleSnapshot, SaturatingConcurrentDoubleHistogram,
};
pub use self::interval_sample::{
    DoubleIntervalSample, FixedIntervalSample, ResizableIntervalSample, SingleWriterDoubleIntervalSample, SingleWriterIntervalSample,
};
pub use self::recorder::{
    DoubleRecorder, DoubleRecorderBuilder, DoubleRecorderWithPolicy, FixedRecorder, FixedRecorderBuilder, ResizableRecorder,
    ResizableRecorderBuilder, SaturatingDoubleRecorder, SaturatingSingleWriterDoubleRecorder, SingleWriterDoubleRecorder,
    SingleWriterDoubleRecorderBuilder, SingleWriterDoubleRecorderWithPolicy, SingleWriterRecorder, SingleWriterRecorderBuilder,
};
pub(crate) use self::resizable_histogram::ResizableStructuralMutation;
pub use self::resizable_histogram::{ResizableConcurrentHistogram, ResizableConcurrentHistogramBuilder, ResizableConcurrentReadView};
pub use self::snapshot::{FixedSnapshot, ResizableSnapshot};
pub use self::static_histogram::{FixedConcurrentHistogram, FixedConcurrentHistogramBuilder};
pub use crate::core::{OverflowPolicy, SaturateOnOverflow, ThrowOnOverflow};
