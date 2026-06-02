use std::error::Error;
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IterationError {
    ConcurrentModification,
}

impl fmt::Display for IterationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IterationError::ConcurrentModification => write!(f, "histogram was modified concurrently during iteration"),
        }
    }
}

impl Error for IterationError {}
