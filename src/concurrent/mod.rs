//! Concurrent histograms and recorder types.
//!
//! Most users should use the recorder constructors in [`recorder`] instead of
//! recording directly into concurrent histograms. Recorders keep writer-side
//! updates lock-free with respect to sampling and expose snapshots through
//! [`ResizableLockingSample`] or [`FixedLockingSample`].

pub(crate) mod concurrent_util;
mod double_histogram;
pub(crate) mod inline_backing_array;
mod locking_sample;
pub(crate) mod recordable_histogram;
pub mod recorder;
mod resizable_histogram;
mod snapshot;
mod static_histogram;
pub(crate) mod writer_reader_phaser;

pub use self::double_histogram::{
    ConcurrentDoubleHistogram, ConcurrentDoubleHistogramBuilder, ConcurrentDoubleHistogramWithPolicy, ConcurrentDoubleReadView,
    ConcurrentDoubleSnapshot, SaturatingConcurrentDoubleHistogram,
};
pub use self::locking_sample::{
    DoubleLockingSample, FixedLockingSample, ResizableLockingSample, SingleWriterDoubleLockingSample, SingleWriterLockingSample,
};
pub use self::recorder::{
    DoubleRecorder, FixedRecorder, ResizableRecorder, SaturatingDoubleRecorder, SaturatingSingleWriterDoubleRecorder,
    SingleWriterDoubleRecorder, SingleWriterRecorder,
};
pub(crate) use self::resizable_histogram::ResizableStructuralMutation;
pub use self::resizable_histogram::{ResizableConcurrentHistogram, ResizableConcurrentHistogramBuilder, ResizableConcurrentReadView};
pub use self::snapshot::{FixedSnapshot, ResizableSnapshot};
pub use self::static_histogram::{FixedConcurrentHistogram, FixedConcurrentHistogramBuilder};
