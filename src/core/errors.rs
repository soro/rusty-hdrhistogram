use std::error::Error;
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CreationError {
    LowIsZero,
    LowGtMax,
    HighLt2Low,
    SignificantValueDigitsExceedsMax,
    CantReprSigDigitsLtLowestDiscernible,
    RequiresExcessiveArrayLen,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SubtractionError {
    ValueOutOfRange,
    CountExceededAtValue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecordError {
    ValueOutOfRangeResizeDisabled,
    ResizeFailed(CreationError),
    DoubleCreationFailed(DoubleCreationError),
    ConcurrentModification,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DoubleCreationError {
    HighestToLowestValueRatioTooSmall,
    HighestToLowestValueRatioTooLarge,
    SignificantValueDigitsExceedsMax,
    InternalHistogramMismatch,
    Internal(CreationError),
}

impl From<DoubleCreationError> for RecordError {
    fn from(err: DoubleCreationError) -> Self {
        RecordError::DoubleCreationFailed(err)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShiftError {
    Underflow,
    Overflow,
}

impl fmt::Display for CreationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CreationError::LowIsZero => write!(f, "lowest discernible value must be greater than zero"),
            CreationError::LowGtMax => write!(f, "lowest discernible value is too large"),
            CreationError::HighLt2Low => write!(f, "highest trackable value must be at least twice the lowest discernible value"),
            CreationError::SignificantValueDigitsExceedsMax => write!(f, "significant value digits must be at most 5"),
            CreationError::CantReprSigDigitsLtLowestDiscernible => {
                write!(
                    f,
                    "cannot represent the requested significant digits at the lowest discernible value"
                )
            }
            CreationError::RequiresExcessiveArrayLen => write!(f, "requested range requires an excessive counts array length"),
        }
    }
}

impl Error for CreationError {}

impl fmt::Display for SubtractionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SubtractionError::ValueOutOfRange => write!(f, "subtrahend contains a value outside the target histogram range"),
            SubtractionError::CountExceededAtValue => write!(f, "subtrahend count exceeds the target count at a value"),
        }
    }
}

impl Error for SubtractionError {}

impl fmt::Display for RecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecordError::ValueOutOfRangeResizeDisabled => write!(f, "value is outside the trackable range and auto-resize is disabled"),
            RecordError::ResizeFailed(err) => write!(f, "failed to resize histogram while recording: {}", err),
            RecordError::DoubleCreationFailed(err) => write!(f, "failed to create internal double histogram while recording: {}", err),
            RecordError::ConcurrentModification => write!(f, "source histogram was modified concurrently while recording"),
        }
    }
}

impl Error for RecordError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            RecordError::ResizeFailed(err) => Some(err),
            RecordError::DoubleCreationFailed(err) => Some(err),
            RecordError::ValueOutOfRangeResizeDisabled | RecordError::ConcurrentModification => None,
        }
    }
}

impl fmt::Display for DoubleCreationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DoubleCreationError::HighestToLowestValueRatioTooSmall => {
                write!(f, "highest-to-lowest value ratio must be at least 2")
            }
            DoubleCreationError::HighestToLowestValueRatioTooLarge => write!(f, "highest-to-lowest value ratio is too large"),
            DoubleCreationError::SignificantValueDigitsExceedsMax => write!(f, "significant value digits must be at most 5"),
            DoubleCreationError::InternalHistogramMismatch => {
                write!(f, "internal integer histogram does not match the double histogram configuration")
            }
            DoubleCreationError::Internal(err) => write!(f, "failed to create internal integer histogram: {}", err),
        }
    }
}

impl Error for DoubleCreationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            DoubleCreationError::Internal(err) => Some(err),
            _ => None,
        }
    }
}

impl fmt::Display for ShiftError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShiftError::Underflow => write!(f, "histogram shift would underflow the trackable range"),
            ShiftError::Overflow => write!(f, "histogram shift would overflow the trackable range"),
        }
    }
}

impl Error for ShiftError {}
