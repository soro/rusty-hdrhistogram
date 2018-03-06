use std::io;
use std::time::SystemTimeError;
use base64;

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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SerializationError {
    ValueNotLEBEncodable,
    BufferCapacityInsufficient,
    CompressedBufferCapacityInsufficient,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeserializationError {
    HistogramCreationFailed(CreationError),
    CountExceedsTypeMax,
    PayloadExceededCountsArrayLength,
    CompressionFormatNotRecognized,
    DecompressionBufferCapacityInsufficient(usize, usize),
    DecompressionFailed,
}

#[derive(Debug)]
pub enum LoggingError {
    IoError(io::Error),
    SerializationError(SerializationError),
    InvalidTime(SystemTimeError),
}

impl From<io::Error> for LoggingError {
    fn from(err: io::Error) -> LoggingError {
        LoggingError::IoError(err)
    }
}

impl From<SerializationError> for LoggingError {
    fn from(err: SerializationError) -> LoggingError {
        LoggingError::SerializationError(err)
    }
}

impl From<SystemTimeError> for LoggingError {
    fn from(err: SystemTimeError) -> LoggingError { LoggingError::InvalidTime(err) }
}

pub enum LogReadError {
    DeserializationError(DeserializationError),
    Base64DecodingError(base64::DecodeError),
}

impl From<DeserializationError> for LogReadError {
    fn from(err: DeserializationError) -> LogReadError {
        LogReadError::DeserializationError(err)
    }
}

impl From<base64::DecodeError> for LogReadError {
    fn from(err: base64::DecodeError) -> LogReadError {
        LogReadError::Base64DecodingError(err)
    }
}