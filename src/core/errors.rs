#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CreationError {
    LowIsZero,
    LowGtMax,
    HighLt2Low,
    SignificantValueDigitsExceedsMax,
    CantReprSigDigitsLtLowestDiscernible,
    CountsArrayLengthMismatch { expected: u32, actual: u32 },
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DoubleCreationError {
    HighestToLowestValueRatioTooSmall,
    HighestToLowestValueRatioTooLarge,
    SignificantValueDigitsExceedsMax,
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
