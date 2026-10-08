use std::error::Error;
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IterationError {
    ConcurrentModification,
    /// The bucket width is zero, negative, non-finite, or too large after
    /// conversion to the histogram's integer units.
    InvalidBucketWidth,
    /// The logarithmic base is non-finite or at most one.
    InvalidLogBase,
}

impl fmt::Display for IterationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IterationError::ConcurrentModification => write!(f, "histogram was modified concurrently during iteration"),
            IterationError::InvalidBucketWidth => write!(f, "bucket width must be finite, positive, and within the integer unit range"),
            IterationError::InvalidLogBase => write!(f, "logarithmic base must be finite and greater than 1"),
        }
    }
}

impl Error for IterationError {}
