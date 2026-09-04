//! Java-compatible HdrHistogram binary encoding and histogram-log helpers.
//!
//! Raw V2 integer and double histogram encoding is always available. Compressed
//! encoding requires `encoding-compression`; Base64 helpers, histogram log
//! reader/writer APIs, and report generation require `encoding-base64`.

use crate::concurrent::{ConcurrentDoubleReadView, ConcurrentDoubleSnapshot};
#[cfg(feature = "encoding-compression")]
use crate::core::HistogramSettings;
use crate::core::{ConstructableHistogram, CreationError, DoubleCreationError, ReadableHistogram};
use crate::st::{DoubleHistogram, Histogram};

pub use crate::core::{EncodableHistogram, OverflowPolicy};

#[cfg(feature = "encoding-base64")]
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
#[cfg(feature = "encoding-base64")]
use chrono::{Local, TimeZone};
#[cfg(feature = "encoding-compression")]
use flate2::{read::ZlibDecoder, read::ZlibEncoder, Compression};
#[cfg(feature = "encoding-base64")]
use std::collections::{BTreeSet, VecDeque};
use std::fmt;
#[cfg(feature = "encoding-base64")]
use std::io::{BufRead, Cursor, Write};
#[cfg(feature = "encoding-compression")]
use std::io::{ErrorKind, Read};

const V0_ENCODING_COOKIE_BASE: u32 = 0x1c849308;
const V0_COMPRESSED_ENCODING_COOKIE_BASE: u32 = 0x1c849309;
const V1_ENCODING_COOKIE_BASE: u32 = 0x1c849301;
const V1_COMPRESSED_ENCODING_COOKIE_BASE: u32 = 0x1c849302;
const V2_ENCODING_COOKIE_BASE: u32 = 0x1c849303;
const V2_COMPRESSED_ENCODING_COOKIE_BASE: u32 = 0x1c849304;

pub const V2_ENCODING_COOKIE: u32 = V2_ENCODING_COOKIE_BASE | 0x10;
pub const V2_COMPRESSED_ENCODING_COOKIE: u32 = V2_COMPRESSED_ENCODING_COOKIE_BASE | 0x10;
pub const DOUBLE_HISTOGRAM_ENCODING_COOKIE: u32 = 0x0c72124e;
pub const DOUBLE_HISTOGRAM_COMPRESSED_ENCODING_COOKIE: u32 = 0x0c72124f;

const V2_MAX_WORD_SIZE_IN_BYTES: u8 = 9;
const ENCODING_HEADER_SIZE: usize = 40;
const V0_ENCODING_HEADER_SIZE: usize = 32;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EncodeError {
    CountTooLarge(u64),
    ValueTooLarge(u64),
    PayloadTooLarge(usize),
    InvalidCompressionLevel(u32),
    InvalidLogLine(String),
    Compression(String),
    Io(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DecodeError {
    UnexpectedEof,
    InvalidCookie(u32),
    UnsupportedCompressedEncoding,
    UnsupportedWordSize(u8),
    InvalidPayloadLength(i32),
    InvalidPayload,
    CountOverflow,
    CountsArrayIndexOutOfBounds(u32),
    Creation(CreationError),
    DoubleCreation(DoubleCreationError),
    InvalidLogLine(String),
    Compression(String),
    Base64(String),
    Io(String),
}

impl From<CreationError> for DecodeError {
    fn from(err: CreationError) -> Self {
        DecodeError::Creation(err)
    }
}

impl From<DoubleCreationError> for DecodeError {
    fn from(err: DoubleCreationError) -> Self {
        DecodeError::DoubleCreation(err)
    }
}

impl From<std::io::Error> for EncodeError {
    fn from(err: std::io::Error) -> Self {
        EncodeError::Io(err.to_string())
    }
}

impl From<std::io::Error> for DecodeError {
    fn from(err: std::io::Error) -> Self {
        DecodeError::Io(err.to_string())
    }
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EncodeError::CountTooLarge(count) => write!(f, "count {} is too large to encode", count),
            EncodeError::ValueTooLarge(value) => write!(f, "value {} is too large to encode", value),
            EncodeError::PayloadTooLarge(length) => write!(f, "encoded payload length {} is too large", length),
            EncodeError::InvalidCompressionLevel(level) => write!(f, "invalid compression level {}", level),
            EncodeError::InvalidLogLine(line) => write!(f, "invalid histogram log line: {}", line),
            EncodeError::Compression(err) => write!(f, "compression failed: {}", err),
            EncodeError::Io(err) => write!(f, "I/O failed: {}", err),
        }
    }
}

impl std::error::Error for EncodeError {}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::UnexpectedEof => write!(f, "unexpected end of encoded histogram data"),
            DecodeError::InvalidCookie(cookie) => write!(f, "invalid histogram encoding cookie 0x{:08x}", cookie),
            DecodeError::UnsupportedCompressedEncoding => write!(f, "compressed histogram encoding is not enabled"),
            DecodeError::UnsupportedWordSize(word_size) => write!(f, "unsupported encoded word size {}", word_size),
            DecodeError::InvalidPayloadLength(length) => write!(f, "invalid encoded payload length {}", length),
            DecodeError::InvalidPayload => write!(f, "invalid encoded histogram payload"),
            DecodeError::CountOverflow => write!(f, "decoded count overflowed the target histogram"),
            DecodeError::CountsArrayIndexOutOfBounds(index) => write!(f, "decoded counts array index {} is out of bounds", index),
            DecodeError::Creation(err) => write!(f, "failed to create decoded integer histogram: {}", err),
            DecodeError::DoubleCreation(err) => write!(f, "failed to create decoded double histogram: {}", err),
            DecodeError::InvalidLogLine(line) => write!(f, "invalid histogram log line: {}", line),
            DecodeError::Compression(err) => write!(f, "compression failed: {}", err),
            DecodeError::Base64(err) => write!(f, "Base64 decoding failed: {}", err),
            DecodeError::Io(err) => write!(f, "I/O failed: {}", err),
        }
    }
}

impl std::error::Error for DecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DecodeError::Creation(err) => Some(err),
            DecodeError::DoubleCreation(err) => Some(err),
            _ => None,
        }
    }
}

pub enum DecodedHistogram {
    Integer(Histogram),
    Double(DoubleHistogram),
}

#[cfg(feature = "encoding-base64")]
pub const HISTOGRAM_LOG_FORMAT_VERSION: &str = "1.3";

#[cfg(feature = "encoding-base64")]
pub const DEFAULT_LOG_MAX_VALUE_UNIT_RATIO: f64 = 1_000_000.0;

#[cfg(feature = "encoding-base64")]
#[allow(clippy::large_enum_variant)]
pub enum HistogramLogRecord {
    StartTime(f64),
    BaseTime(f64),
    Comment(String),
    Legend,
    Interval(HistogramLogEntry),
}

#[cfg(feature = "encoding-base64")]
#[derive(Clone, Debug, PartialEq)]
pub enum HistogramLogScannedRecord {
    StartTime(f64),
    BaseTime(f64),
    Comment(String),
    Legend,
    Interval(HistogramLogScannedInterval),
}

#[cfg(feature = "encoding-base64")]
pub struct HistogramLogEntry {
    pub tag: Option<String>,
    pub start_timestamp_sec: f64,
    pub interval_length_sec: f64,
    pub max_value: f64,
    pub histogram: DecodedHistogram,
}

#[cfg(feature = "encoding-base64")]
#[derive(Clone, Debug, PartialEq)]
pub struct HistogramLogScannedInterval {
    pub tag: Option<String>,
    pub start_timestamp_sec: f64,
    pub interval_length_sec: f64,
    pub max_value: f64,
    pub absolute_start_time_sec: f64,
    pub absolute_end_time_sec: f64,
    pub relative_start_time_sec: f64,
    pub relative_end_time_sec: f64,
    pub compressed_histogram_base64: String,
}

#[cfg(feature = "encoding-base64")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HistogramLogTagFilter {
    Untagged,
    Tag(String),
    Any,
}

#[cfg(feature = "encoding-base64")]
#[derive(Clone, Debug, PartialEq)]
pub struct HistogramLogReportConfig {
    pub tag_filter: HistogramLogTagFilter,
    pub range_start_time_sec: f64,
    pub range_end_time_sec: f64,
    pub output_value_unit_ratio: f64,
    pub percentile_ticks_per_half_distance: u32,
    pub csv: bool,
    pub moving_window: Option<HistogramLogMovingWindowConfig>,
    pub expected_interval_for_coordinated_omission_correction: f64,
    pub processor_time_range_headers: bool,
    pub processor_start_time_header: bool,
}

#[cfg(feature = "encoding-base64")]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HistogramLogMovingWindowConfig {
    pub percentile_to_report: f64,
    pub length_sec: f64,
}

#[cfg(feature = "encoding-base64")]
#[derive(Clone, Debug, PartialEq)]
pub struct HistogramLogReport {
    pub start_time_sec: f64,
    pub base_time_sec: f64,
    pub processed_interval_count: usize,
    pub tags: Vec<Option<String>>,
    pub interval_log: String,
    pub moving_window_log: Option<String>,
    pub percentile_distribution: String,
}

#[cfg(feature = "encoding-base64")]
#[derive(Clone, Debug, PartialEq)]
pub struct HistogramLogReportSummary {
    pub start_time_sec: f64,
    pub base_time_sec: f64,
    pub processed_interval_count: usize,
    pub tags: Vec<Option<String>>,
}

#[cfg(feature = "encoding-base64")]
pub struct HistogramLogInterval {
    pub tag: Option<String>,
    pub start_timestamp_sec: f64,
    pub interval_length_sec: f64,
    pub max_value: f64,
    pub absolute_start_time_sec: f64,
    pub absolute_end_time_sec: f64,
    pub relative_start_time_sec: f64,
    pub relative_end_time_sec: f64,
    pub histogram: DecodedHistogram,
}

#[cfg(feature = "encoding-base64")]
#[derive(Clone, Copy, Debug, PartialEq)]
struct HistogramLogIntervalTiming {
    absolute_start_time_sec: f64,
    absolute_end_time_sec: f64,
    relative_start_time_sec: f64,
    relative_end_time_sec: f64,
}

/// Streaming scanner for Java-compatible histogram logs.
///
/// The scanner preserves Java's start-time and base-time inference rules but
/// does not decode compressed histogram payloads unless callers explicitly call
/// [`HistogramLogScannedInterval::decode`] or
/// [`HistogramLogScannedInterval::decode_histogram`]. This is the cheapest API
/// for tag listing, metadata inspection, and range/tag filtering.
///
#[cfg(feature = "encoding-base64")]
pub struct HistogramLogScanner<R> {
    reader: R,
    observed_start_time: bool,
    observed_base_time: bool,
    start_time_sec: f64,
    base_time_sec: f64,
}

#[cfg(feature = "encoding-base64")]
/// Streaming reader for Java-compatible histogram logs.
///
/// The reader builds on [`HistogramLogScanner`] and decodes each interval before
/// returning it. Use the scanner directly when payload decoding should be
/// deferred until after metadata filtering.
pub struct HistogramLogReader<R> {
    scanner: HistogramLogScanner<R>,
}

/// Streaming writer for Java-compatible histogram logs.
///
/// The writer owns any `Write` target and delegates interval encoding to the
/// same Base64/compressed V2 helpers used by the line-level APIs. If a base time
/// is set, interval timestamps are emitted relative to it.
#[cfg(feature = "encoding-base64")]
pub struct HistogramLogWriter<W> {
    writer: W,
    base_time_sec: Option<f64>,
    max_value_unit_ratio: f64,
}

#[cfg(feature = "encoding-base64")]
impl Default for HistogramLogReportConfig {
    fn default() -> Self {
        HistogramLogReportConfig {
            tag_filter: HistogramLogTagFilter::Untagged,
            range_start_time_sec: 0.0,
            range_end_time_sec: f64::MAX,
            output_value_unit_ratio: DEFAULT_LOG_MAX_VALUE_UNIT_RATIO,
            percentile_ticks_per_half_distance: 5,
            csv: false,
            moving_window: None,
            expected_interval_for_coordinated_omission_correction: 0.0,
            processor_time_range_headers: false,
            processor_start_time_header: false,
        }
    }
}

#[cfg(feature = "encoding-base64")]
impl HistogramLogScannedRecord {
    pub fn decode(&self) -> Result<HistogramLogRecord, DecodeError> {
        match self {
            HistogramLogScannedRecord::StartTime(value) => Ok(HistogramLogRecord::StartTime(*value)),
            HistogramLogScannedRecord::BaseTime(value) => Ok(HistogramLogRecord::BaseTime(*value)),
            HistogramLogScannedRecord::Comment(comment) => Ok(HistogramLogRecord::Comment(comment.clone())),
            HistogramLogScannedRecord::Legend => Ok(HistogramLogRecord::Legend),
            HistogramLogScannedRecord::Interval(interval) => interval.decode().map(HistogramLogRecord::Interval),
        }
    }
}

#[cfg(feature = "encoding-base64")]
impl HistogramLogScannedInterval {
    pub fn decode(&self) -> Result<HistogramLogEntry, DecodeError> {
        Ok(HistogramLogEntry {
            tag: self.tag.clone(),
            start_timestamp_sec: self.start_timestamp_sec,
            interval_length_sec: self.interval_length_sec,
            max_value: self.max_value,
            histogram: self.decode_histogram()?,
        })
    }

    pub fn decode_histogram(&self) -> Result<DecodedHistogram, DecodeError> {
        decode_histogram_log_payload(&self.compressed_histogram_base64)
    }

    pub fn log_line(&self) -> String {
        match self.tag.as_deref() {
            Some(tag) => format!(
                "Tag={},{:.3},{:.3},{:.3},{}\n",
                tag, self.start_timestamp_sec, self.interval_length_sec, self.max_value, self.compressed_histogram_base64
            ),
            None => format!(
                "{:.3},{:.3},{:.3},{}\n",
                self.start_timestamp_sec, self.interval_length_sec, self.max_value, self.compressed_histogram_base64
            ),
        }
    }
}

#[cfg(feature = "encoding-base64")]
impl<R: BufRead> HistogramLogScanner<R> {
    pub fn new(reader: R) -> Self {
        HistogramLogScanner {
            reader,
            observed_start_time: false,
            observed_base_time: false,
            start_time_sec: 0.0,
            base_time_sec: 0.0,
        }
    }

    pub fn start_time_sec(&self) -> f64 {
        self.start_time_sec
    }

    pub fn base_time_sec(&self) -> f64 {
        self.base_time_sec
    }

    pub fn observed_start_time(&self) -> bool {
        self.observed_start_time
    }

    pub fn observed_base_time(&self) -> bool {
        self.observed_base_time
    }

    pub fn into_inner(self) -> R {
        self.reader
    }

    pub fn next_record(&mut self) -> Result<Option<HistogramLogScannedRecord>, DecodeError> {
        let Some(mut record) = self.read_next_scanned_record()? else {
            return Ok(None);
        };
        self.observe_record(&mut record);
        Ok(Some(record))
    }

    pub fn next_interval(&mut self) -> Result<Option<HistogramLogScannedInterval>, DecodeError> {
        while let Some(record) = self.next_record()? {
            if let HistogramLogScannedRecord::Interval(interval) = record {
                return Ok(Some(interval));
            }
        }
        Ok(None)
    }

    pub fn next_interval_in_range(
        &mut self,
        range_start_time_sec: f64,
        range_end_time_sec: f64,
    ) -> Result<Option<HistogramLogScannedInterval>, DecodeError> {
        self.next_filtered_interval(range_start_time_sec, range_end_time_sec, false)
    }

    pub fn next_absolute_interval_in_range(
        &mut self,
        range_start_time_sec: f64,
        range_end_time_sec: f64,
    ) -> Result<Option<HistogramLogScannedInterval>, DecodeError> {
        self.next_filtered_interval(range_start_time_sec, range_end_time_sec, true)
    }

    fn next_filtered_interval(
        &mut self,
        range_start_time_sec: f64,
        range_end_time_sec: f64,
        absolute: bool,
    ) -> Result<Option<HistogramLogScannedInterval>, DecodeError> {
        validate_time_range(range_start_time_sec, range_end_time_sec)?;
        while let Some(interval) = self.next_interval()? {
            let timestamp = if absolute {
                interval.absolute_start_time_sec
            } else {
                interval.relative_start_time_sec
            };
            if timestamp < range_start_time_sec {
                continue;
            }
            if timestamp > range_end_time_sec {
                return Ok(None);
            }
            return Ok(Some(interval));
        }
        Ok(None)
    }

    fn read_next_scanned_record(&mut self) -> Result<Option<HistogramLogScannedRecord>, DecodeError> {
        let mut line = String::new();
        loop {
            line.clear();
            let bytes = self.reader.read_line(&mut line)?;
            if bytes == 0 {
                return Ok(None);
            }
            if let Some(record) = scan_histogram_log_line(&line)? {
                return Ok(Some(record));
            }
        }
    }

    fn observe_record(&mut self, record: &mut HistogramLogScannedRecord) {
        match record {
            HistogramLogScannedRecord::StartTime(value) => {
                self.start_time_sec = *value;
                self.observed_start_time = true;
            }
            HistogramLogScannedRecord::BaseTime(value) => {
                self.base_time_sec = *value;
                self.observed_base_time = true;
            }
            HistogramLogScannedRecord::Interval(interval) => {
                let timing = self.observe_interval_timing(interval.start_timestamp_sec, interval.interval_length_sec);
                interval.absolute_start_time_sec = timing.absolute_start_time_sec;
                interval.absolute_end_time_sec = timing.absolute_end_time_sec;
                interval.relative_start_time_sec = timing.relative_start_time_sec;
                interval.relative_end_time_sec = timing.relative_end_time_sec;
            }
            HistogramLogScannedRecord::Comment(_) | HistogramLogScannedRecord::Legend => {}
        }
    }

    fn observe_interval_timing(&mut self, start_timestamp_sec: f64, interval_length_sec: f64) -> HistogramLogIntervalTiming {
        if !self.observed_start_time {
            self.start_time_sec = start_timestamp_sec;
            self.observed_start_time = true;
        }
        if !self.observed_base_time {
            self.base_time_sec = if start_timestamp_sec < self.start_time_sec - (365.0 * 24.0 * 3600.0) {
                self.start_time_sec
            } else {
                0.0
            };
            self.observed_base_time = true;
        }

        let absolute_start_time_sec = start_timestamp_sec + self.base_time_sec;
        let absolute_end_time_sec = absolute_start_time_sec + interval_length_sec;
        HistogramLogIntervalTiming {
            absolute_start_time_sec,
            absolute_end_time_sec,
            relative_start_time_sec: absolute_start_time_sec - self.start_time_sec,
            relative_end_time_sec: absolute_end_time_sec - self.start_time_sec,
        }
    }
}

#[cfg(feature = "encoding-base64")]
impl<R: BufRead> HistogramLogReader<R> {
    pub fn new(reader: R) -> Self {
        HistogramLogReader {
            scanner: HistogramLogScanner::new(reader),
        }
    }

    pub fn start_time_sec(&self) -> f64 {
        self.scanner.start_time_sec()
    }

    pub fn base_time_sec(&self) -> f64 {
        self.scanner.base_time_sec()
    }

    pub fn observed_start_time(&self) -> bool {
        self.scanner.observed_start_time()
    }

    pub fn observed_base_time(&self) -> bool {
        self.scanner.observed_base_time()
    }

    pub fn into_inner(self) -> R {
        self.scanner.into_inner()
    }

    pub fn next_record(&mut self) -> Result<Option<HistogramLogRecord>, DecodeError> {
        match self.scanner.next_record()? {
            Some(record) => record.decode().map(Some),
            None => Ok(None),
        }
    }

    pub fn next_interval(&mut self) -> Result<Option<HistogramLogInterval>, DecodeError> {
        match self.scanner.next_interval()? {
            Some(interval) => decoded_interval_from_scanned(interval).map(Some),
            None => Ok(None),
        }
    }

    pub fn next_interval_in_range(
        &mut self,
        range_start_time_sec: f64,
        range_end_time_sec: f64,
    ) -> Result<Option<HistogramLogInterval>, DecodeError> {
        match self.scanner.next_interval_in_range(range_start_time_sec, range_end_time_sec)? {
            Some(interval) => decoded_interval_from_scanned(interval).map(Some),
            None => Ok(None),
        }
    }

    pub fn next_absolute_interval_in_range(
        &mut self,
        range_start_time_sec: f64,
        range_end_time_sec: f64,
    ) -> Result<Option<HistogramLogInterval>, DecodeError> {
        match self
            .scanner
            .next_absolute_interval_in_range(range_start_time_sec, range_end_time_sec)?
        {
            Some(interval) => decoded_interval_from_scanned(interval).map(Some),
            None => Ok(None),
        }
    }
}

#[cfg(feature = "encoding-base64")]
impl<W: Write> HistogramLogWriter<W> {
    pub fn new(writer: W) -> Self {
        HistogramLogWriter {
            writer,
            base_time_sec: None,
            max_value_unit_ratio: DEFAULT_LOG_MAX_VALUE_UNIT_RATIO,
        }
    }

    pub fn with_max_value_unit_ratio(writer: W, max_value_unit_ratio: f64) -> Result<Self, EncodeError> {
        Ok(HistogramLogWriter {
            writer,
            base_time_sec: None,
            max_value_unit_ratio: checked_max_value_unit_ratio(max_value_unit_ratio)?,
        })
    }

    pub fn into_inner(self) -> W {
        self.writer
    }

    pub fn base_time_sec(&self) -> Option<f64> {
        self.base_time_sec
    }

    pub fn set_base_time(&mut self, base_time_sec: f64) -> Result<(), EncodeError> {
        if !base_time_sec.is_finite() {
            return Err(EncodeError::InvalidLogLine("base time must be finite".to_string()));
        }
        self.base_time_sec = Some(base_time_sec);
        Ok(())
    }

    pub fn clear_base_time(&mut self) {
        self.base_time_sec = None;
    }

    pub fn write_format_version(&mut self) -> Result<(), EncodeError> {
        self.writer
            .write_all(histogram_log_format_version_line().as_bytes())
            .map_err(EncodeError::from)
    }

    pub fn write_legend(&mut self) -> Result<(), EncodeError> {
        self.writer
            .write_all(histogram_log_legend_line().as_bytes())
            .map_err(EncodeError::from)
    }

    pub fn write_start_time(&mut self, start_time_sec: f64) -> Result<(), EncodeError> {
        if !start_time_sec.is_finite() {
            return Err(EncodeError::InvalidLogLine("start time must be finite".to_string()));
        }
        self.writer
            .write_all(histogram_log_start_time_line(start_time_sec).as_bytes())
            .map_err(EncodeError::from)
    }

    pub fn write_base_time(&mut self, base_time_sec: f64) -> Result<(), EncodeError> {
        self.set_base_time(base_time_sec)?;
        self.writer
            .write_all(histogram_log_base_time_line(base_time_sec).as_bytes())
            .map_err(EncodeError::from)
    }

    pub fn write_comment(&mut self, comment: &str) -> Result<(), EncodeError> {
        if comment.contains('\n') || comment.contains('\r') {
            return Err(EncodeError::InvalidLogLine("comment cannot contain line breaks".to_string()));
        }
        writeln!(self.writer, "#{}", comment).map_err(EncodeError::from)
    }

    pub fn write_interval<H: EncodableHistogram>(
        &mut self,
        histogram: &H,
        start_timestamp_sec: f64,
        end_timestamp_sec: f64,
    ) -> Result<(), EncodeError> {
        let (start_timestamp_sec, end_timestamp_sec) = self.relative_interval_times(start_timestamp_sec, end_timestamp_sec);
        let line = encode_histogram_log_line_with_max_value_unit_ratio(
            histogram,
            start_timestamp_sec,
            end_timestamp_sec,
            self.max_value_unit_ratio,
        )?;
        self.writer.write_all(line.as_bytes()).map_err(EncodeError::from)
    }

    pub fn write_double_interval<P: OverflowPolicy>(
        &mut self,
        histogram: &DoubleHistogram<P>,
        start_timestamp_sec: f64,
        end_timestamp_sec: f64,
    ) -> Result<(), EncodeError> {
        let (start_timestamp_sec, end_timestamp_sec) = self.relative_interval_times(start_timestamp_sec, end_timestamp_sec);
        let line = encode_double_histogram_log_line_with_max_value_unit_ratio(
            histogram,
            start_timestamp_sec,
            end_timestamp_sec,
            self.max_value_unit_ratio,
        )?;
        self.writer.write_all(line.as_bytes()).map_err(EncodeError::from)
    }

    pub fn write_concurrent_double_read_view_interval(
        &mut self,
        histogram: &ConcurrentDoubleReadView<'_>,
        start_timestamp_sec: f64,
        end_timestamp_sec: f64,
    ) -> Result<(), EncodeError> {
        let (start_timestamp_sec, end_timestamp_sec) = self.relative_interval_times(start_timestamp_sec, end_timestamp_sec);
        let line = encode_concurrent_double_read_view_log_line_with_max_value_unit_ratio(
            histogram,
            start_timestamp_sec,
            end_timestamp_sec,
            self.max_value_unit_ratio,
        )?;
        self.writer.write_all(line.as_bytes()).map_err(EncodeError::from)
    }

    pub fn write_concurrent_double_snapshot_interval<P: OverflowPolicy>(
        &mut self,
        histogram: &ConcurrentDoubleSnapshot<'_, P>,
        start_timestamp_sec: f64,
        end_timestamp_sec: f64,
    ) -> Result<(), EncodeError> {
        let (start_timestamp_sec, end_timestamp_sec) = self.relative_interval_times(start_timestamp_sec, end_timestamp_sec);
        let line = encode_concurrent_double_snapshot_log_line_with_max_value_unit_ratio(
            histogram,
            start_timestamp_sec,
            end_timestamp_sec,
            self.max_value_unit_ratio,
        )?;
        self.writer.write_all(line.as_bytes()).map_err(EncodeError::from)
    }

    pub fn flush(&mut self) -> Result<(), EncodeError> {
        self.writer.flush().map_err(EncodeError::from)
    }

    fn relative_interval_times(&self, start_timestamp_sec: f64, end_timestamp_sec: f64) -> (f64, f64) {
        if let Some(base_time_sec) = self.base_time_sec {
            (start_timestamp_sec - base_time_sec, end_timestamp_sec - base_time_sec)
        } else {
            (start_timestamp_sec, end_timestamp_sec)
        }
    }
}

pub fn encode_histogram_v2<H: EncodableHistogram>(histogram: &H) -> Result<Vec<u8>, EncodeError> {
    let settings = histogram.settings();
    let mut encoded = Vec::with_capacity(settings.v2_encoding_capacity_for_value(histogram.get_max_value()));

    write_u32(&mut encoded, V2_ENCODING_COOKIE);
    write_i32(&mut encoded, 0);
    write_i32(&mut encoded, histogram.normalizing_index_offset());
    write_i32(
        &mut encoded,
        i32::try_from(settings.number_of_significant_value_digits)
            .map_err(|_| EncodeError::ValueTooLarge(settings.number_of_significant_value_digits as u64))?,
    );
    write_non_negative_i64(&mut encoded, settings.lowest_discernible_value)?;
    write_non_negative_i64(&mut encoded, settings.highest_trackable_value)?;
    write_f64(&mut encoded, histogram.integer_to_double_value_conversion_ratio());

    let payload_start = encoded.len();
    encode_counts_payload(histogram, &mut encoded)?;
    let payload_length = encoded.len() - payload_start;
    if payload_length > i32::MAX as usize {
        return Err(EncodeError::PayloadTooLarge(payload_length));
    }
    encoded[4..8].copy_from_slice(&(payload_length as i32).to_be_bytes());

    Ok(encoded)
}

pub fn decode(bytes: &[u8]) -> Result<DecodedHistogram, DecodeError> {
    let cookie = peek_cookie(bytes)?;
    if is_histogram_encoding_cookie(cookie) {
        return decode_histogram(bytes).map(DecodedHistogram::Integer);
    }
    if cookie == DOUBLE_HISTOGRAM_ENCODING_COOKIE {
        return decode_double_histogram_v2(bytes).map(DecodedHistogram::Double);
    }
    if is_histogram_compressed_encoding_cookie(cookie) {
        #[cfg(feature = "encoding-compression")]
        {
            return decode_histogram_compressed(bytes).map(DecodedHistogram::Integer);
        }
        #[cfg(not(feature = "encoding-compression"))]
        {
            return Err(DecodeError::UnsupportedCompressedEncoding);
        }
    }
    if cookie == DOUBLE_HISTOGRAM_COMPRESSED_ENCODING_COOKIE {
        #[cfg(feature = "encoding-compression")]
        {
            return decode_double_histogram_compressed(bytes).map(DecodedHistogram::Double);
        }
        #[cfg(not(feature = "encoding-compression"))]
        {
            return Err(DecodeError::UnsupportedCompressedEncoding);
        }
    }

    Err(DecodeError::InvalidCookie(cookie))
}

pub fn decode_histogram(bytes: &[u8]) -> Result<Histogram, DecodeError> {
    decode_histogram_with_min_highest_trackable_value(bytes, 0)
}

pub fn decode_histogram_v2(bytes: &[u8]) -> Result<Histogram, DecodeError> {
    let cookie = peek_cookie(bytes)?;
    if cookie_base(cookie) != V2_ENCODING_COOKIE_BASE {
        return Err(DecodeError::InvalidCookie(cookie));
    }
    decode_histogram(bytes)
}

pub fn decode_histogram_with_min_highest_trackable_value(bytes: &[u8], min_highest_trackable_value: u64) -> Result<Histogram, DecodeError> {
    let mut reader = Reader::new(bytes);
    let cookie = reader.read_u32()?;
    let base = cookie_base(cookie);

    let (
        payload,
        word_size,
        normalizing_index_offset,
        number_of_significant_value_digits,
        lowest_discernible_value,
        highest_trackable_value,
        integer_to_double_value_conversion_ratio,
    ) = match base {
        V2_ENCODING_COOKIE_BASE | V1_ENCODING_COOKIE_BASE => {
            let word_size = word_size_from_cookie(cookie);
            if base == V2_ENCODING_COOKIE_BASE && word_size != V2_MAX_WORD_SIZE_IN_BYTES {
                return Err(DecodeError::UnsupportedWordSize(word_size));
            }
            if base == V1_ENCODING_COOKIE_BASE && !matches!(word_size, 2 | 4 | 8) {
                return Err(DecodeError::UnsupportedWordSize(word_size));
            }

            let payload_length = reader.read_i32()?;
            if payload_length < 0 {
                return Err(DecodeError::InvalidPayloadLength(payload_length));
            }
            let normalizing_index_offset = reader.read_i32()?;
            let number_of_significant_value_digits = reader.read_i32()?;
            let lowest_discernible_value = reader.read_non_negative_i64()?;
            let highest_trackable_value = reader.read_non_negative_i64()?;
            let integer_to_double_value_conversion_ratio = reader.read_f64()?;
            let payload = reader.take(payload_length as usize)?;

            (
                payload,
                word_size,
                normalizing_index_offset,
                number_of_significant_value_digits,
                lowest_discernible_value,
                highest_trackable_value,
                integer_to_double_value_conversion_ratio,
            )
        }
        V0_ENCODING_COOKIE_BASE => {
            let word_size = word_size_from_cookie(cookie);
            if !matches!(word_size, 2 | 4 | 8) {
                return Err(DecodeError::UnsupportedWordSize(word_size));
            }

            let number_of_significant_value_digits = reader.read_i32()?;
            let lowest_discernible_value = reader.read_non_negative_i64()?;
            let highest_trackable_value = reader.read_non_negative_i64()?;
            let _total_count = reader.read_i64()?;
            let payload = reader.remaining();

            (
                payload,
                word_size,
                0,
                number_of_significant_value_digits,
                lowest_discernible_value,
                highest_trackable_value,
                1.0,
            )
        }
        _ => return Err(DecodeError::InvalidCookie(cookie)),
    };

    if !integer_to_double_value_conversion_ratio.is_finite() || integer_to_double_value_conversion_ratio <= 0.0 {
        return Err(DecodeError::InvalidPayload);
    }
    if number_of_significant_value_digits < 0 || number_of_significant_value_digits > u8::MAX as i32 {
        return Err(DecodeError::InvalidPayload);
    }

    let highest_trackable_value = highest_trackable_value.max(min_highest_trackable_value);
    let mut histogram = Histogram::with_low_high_sigvdig(
        lowest_discernible_value,
        highest_trackable_value,
        number_of_significant_value_digits as u8,
    )?;
    histogram.set_integer_to_double_value_conversion_ratio(integer_to_double_value_conversion_ratio);
    histogram.set_normalizing_index_offset(normalizing_index_offset);
    histogram.set_auto_resize(true);

    decode_counts_payload(payload, word_size, &mut histogram)?;
    ConstructableHistogram::establish_internal_tracking_values(&mut histogram);

    Ok(histogram)
}

fn encode_double_histogram_v2_from_integer<H: EncodableHistogram>(
    integer_histogram: &H,
    number_of_significant_value_digits: u8,
    highest_to_lowest_value_ratio: u64,
) -> Result<Vec<u8>, EncodeError> {
    let integer_encoding = encode_histogram_v2(integer_histogram)?;
    let mut encoded = Vec::with_capacity(16 + integer_encoding.len());
    write_u32(&mut encoded, DOUBLE_HISTOGRAM_ENCODING_COOKIE);
    write_i32(&mut encoded, i32::from(number_of_significant_value_digits));
    write_non_negative_i64(&mut encoded, highest_to_lowest_value_ratio)?;
    encoded.extend_from_slice(&integer_encoding);
    Ok(encoded)
}

pub fn encode_double_histogram_v2<P: OverflowPolicy>(histogram: &DoubleHistogram<P>) -> Result<Vec<u8>, EncodeError> {
    encode_double_histogram_v2_from_integer(
        histogram.integer_histogram(),
        histogram.get_number_of_significant_value_digits(),
        histogram.get_highest_to_lowest_value_ratio(),
    )
}

pub fn encode_concurrent_double_read_view_v2(histogram: &ConcurrentDoubleReadView<'_>) -> Result<Vec<u8>, EncodeError> {
    encode_double_histogram_v2_from_integer(
        histogram,
        histogram.get_number_of_significant_value_digits(),
        histogram.get_highest_to_lowest_value_ratio(),
    )
}

pub fn encode_concurrent_double_snapshot_v2<P: OverflowPolicy>(
    histogram: &ConcurrentDoubleSnapshot<'_, P>,
) -> Result<Vec<u8>, EncodeError> {
    let view = histogram.read_view();
    encode_concurrent_double_read_view_v2(&view)
}

pub fn decode_double_histogram_v2(bytes: &[u8]) -> Result<DoubleHistogram, DecodeError> {
    let mut reader = Reader::new(bytes);
    let cookie = reader.read_u32()?;
    if cookie != DOUBLE_HISTOGRAM_ENCODING_COOKIE {
        return Err(DecodeError::InvalidCookie(cookie));
    }

    let number_of_significant_value_digits = read_significant_value_digits(&mut reader)?;
    let configured_highest_to_lowest_value_ratio = reader.read_non_negative_i64()?;
    let integer_histogram =
        decode_histogram_with_min_highest_trackable_value(reader.remaining(), configured_highest_to_lowest_value_ratio)?;

    DoubleHistogram::from_integer_histogram(
        configured_highest_to_lowest_value_ratio,
        number_of_significant_value_digits,
        integer_histogram,
    )
    .map_err(DecodeError::DoubleCreation)
}

#[cfg(feature = "encoding-compression")]
pub fn encode_histogram_compressed<H: EncodableHistogram>(histogram: &H) -> Result<Vec<u8>, EncodeError> {
    let raw = encode_histogram_v2(histogram)?;
    encode_compressed_payload(V2_COMPRESSED_ENCODING_COOKIE, &raw, Compression::default())
}

#[cfg(feature = "encoding-compression")]
pub fn encode_histogram_compressed_with_level<H: EncodableHistogram>(
    histogram: &H,
    compression_level: u32,
) -> Result<Vec<u8>, EncodeError> {
    if compression_level > 9 {
        return Err(EncodeError::InvalidCompressionLevel(compression_level));
    }
    let raw = encode_histogram_v2(histogram)?;
    encode_compressed_payload(V2_COMPRESSED_ENCODING_COOKIE, &raw, Compression::new(compression_level))
}

#[cfg(feature = "encoding-compression")]
pub fn decode_histogram_compressed(bytes: &[u8]) -> Result<Histogram, DecodeError> {
    decode_histogram_compressed_with_min_highest_trackable_value(bytes, 0)
}

#[cfg(feature = "encoding-compression")]
pub fn decode_histogram_compressed_with_min_highest_trackable_value(
    bytes: &[u8],
    min_highest_trackable_value: u64,
) -> Result<Histogram, DecodeError> {
    let mut reader = Reader::new(bytes);
    let cookie = reader.read_u32()?;
    if !is_histogram_compressed_encoding_cookie(cookie) {
        return Err(DecodeError::InvalidCookie(cookie));
    }
    let compressed_length = reader.read_i32()?;
    if compressed_length < 0 {
        return Err(DecodeError::InvalidPayloadLength(compressed_length));
    }
    let compressed = reader.take(compressed_length as usize)?;
    let raw = decompress_histogram_raw(cookie, compressed, min_highest_trackable_value)?;
    decode_histogram_with_min_highest_trackable_value(&raw, min_highest_trackable_value)
}

#[cfg(feature = "encoding-compression")]
fn encode_double_histogram_compressed_from_integer<H: EncodableHistogram>(
    integer_histogram: &H,
    number_of_significant_value_digits: u8,
    highest_to_lowest_value_ratio: u64,
) -> Result<Vec<u8>, EncodeError> {
    let integer_encoding = encode_histogram_compressed(integer_histogram)?;
    let mut encoded = Vec::with_capacity(16 + integer_encoding.len());
    write_u32(&mut encoded, DOUBLE_HISTOGRAM_COMPRESSED_ENCODING_COOKIE);
    write_i32(&mut encoded, i32::from(number_of_significant_value_digits));
    write_non_negative_i64(&mut encoded, highest_to_lowest_value_ratio)?;
    encoded.extend_from_slice(&integer_encoding);
    Ok(encoded)
}

#[cfg(feature = "encoding-compression")]
pub fn encode_double_histogram_compressed<P: OverflowPolicy>(histogram: &DoubleHistogram<P>) -> Result<Vec<u8>, EncodeError> {
    encode_double_histogram_compressed_from_integer(
        histogram.integer_histogram(),
        histogram.get_number_of_significant_value_digits(),
        histogram.get_highest_to_lowest_value_ratio(),
    )
}

#[cfg(feature = "encoding-compression")]
pub fn encode_concurrent_double_read_view_compressed(histogram: &ConcurrentDoubleReadView<'_>) -> Result<Vec<u8>, EncodeError> {
    encode_double_histogram_compressed_from_integer(
        histogram,
        histogram.get_number_of_significant_value_digits(),
        histogram.get_highest_to_lowest_value_ratio(),
    )
}

#[cfg(feature = "encoding-compression")]
pub fn encode_concurrent_double_snapshot_compressed<P: OverflowPolicy>(
    histogram: &ConcurrentDoubleSnapshot<'_, P>,
) -> Result<Vec<u8>, EncodeError> {
    let view = histogram.read_view();
    encode_concurrent_double_read_view_compressed(&view)
}

#[cfg(feature = "encoding-compression")]
fn encode_double_histogram_compressed_with_level_from_integer<H: EncodableHistogram>(
    integer_histogram: &H,
    number_of_significant_value_digits: u8,
    highest_to_lowest_value_ratio: u64,
    compression_level: u32,
) -> Result<Vec<u8>, EncodeError> {
    if compression_level > 9 {
        return Err(EncodeError::InvalidCompressionLevel(compression_level));
    }
    let integer_encoding = encode_histogram_compressed_with_level(integer_histogram, compression_level)?;
    let mut encoded = Vec::with_capacity(16 + integer_encoding.len());
    write_u32(&mut encoded, DOUBLE_HISTOGRAM_COMPRESSED_ENCODING_COOKIE);
    write_i32(&mut encoded, i32::from(number_of_significant_value_digits));
    write_non_negative_i64(&mut encoded, highest_to_lowest_value_ratio)?;
    encoded.extend_from_slice(&integer_encoding);
    Ok(encoded)
}

#[cfg(feature = "encoding-compression")]
pub fn encode_double_histogram_compressed_with_level<P: OverflowPolicy>(
    histogram: &DoubleHistogram<P>,
    compression_level: u32,
) -> Result<Vec<u8>, EncodeError> {
    encode_double_histogram_compressed_with_level_from_integer(
        histogram.integer_histogram(),
        histogram.get_number_of_significant_value_digits(),
        histogram.get_highest_to_lowest_value_ratio(),
        compression_level,
    )
}

#[cfg(feature = "encoding-compression")]
pub fn encode_concurrent_double_read_view_compressed_with_level(
    histogram: &ConcurrentDoubleReadView<'_>,
    compression_level: u32,
) -> Result<Vec<u8>, EncodeError> {
    encode_double_histogram_compressed_with_level_from_integer(
        histogram,
        histogram.get_number_of_significant_value_digits(),
        histogram.get_highest_to_lowest_value_ratio(),
        compression_level,
    )
}

#[cfg(feature = "encoding-compression")]
pub fn encode_concurrent_double_snapshot_compressed_with_level<P: OverflowPolicy>(
    histogram: &ConcurrentDoubleSnapshot<'_, P>,
    compression_level: u32,
) -> Result<Vec<u8>, EncodeError> {
    let view = histogram.read_view();
    encode_concurrent_double_read_view_compressed_with_level(&view, compression_level)
}

#[cfg(feature = "encoding-compression")]
pub fn decode_double_histogram_compressed(bytes: &[u8]) -> Result<DoubleHistogram, DecodeError> {
    let mut reader = Reader::new(bytes);
    let cookie = reader.read_u32()?;
    if cookie != DOUBLE_HISTOGRAM_COMPRESSED_ENCODING_COOKIE {
        return Err(DecodeError::InvalidCookie(cookie));
    }

    let number_of_significant_value_digits = read_significant_value_digits(&mut reader)?;
    let configured_highest_to_lowest_value_ratio = reader.read_non_negative_i64()?;
    let integer_histogram =
        decode_histogram_compressed_with_min_highest_trackable_value(reader.remaining(), configured_highest_to_lowest_value_ratio)?;

    DoubleHistogram::from_integer_histogram(
        configured_highest_to_lowest_value_ratio,
        number_of_significant_value_digits,
        integer_histogram,
    )
    .map_err(DecodeError::DoubleCreation)
}

#[cfg(feature = "encoding-base64")]
pub fn encode_histogram_base64<H: EncodableHistogram>(histogram: &H) -> Result<String, EncodeError> {
    Ok(BASE64_STANDARD.encode(encode_histogram_compressed(histogram)?))
}

#[cfg(feature = "encoding-base64")]
pub fn decode_histogram_base64(encoded: &str) -> Result<Histogram, DecodeError> {
    let compressed = BASE64_STANDARD
        .decode(encoded)
        .map_err(|err| DecodeError::Base64(err.to_string()))?;
    decode_histogram_compressed(&compressed)
}

#[cfg(feature = "encoding-base64")]
pub fn encode_double_histogram_base64<P: OverflowPolicy>(histogram: &DoubleHistogram<P>) -> Result<String, EncodeError> {
    Ok(BASE64_STANDARD.encode(encode_double_histogram_compressed(histogram)?))
}

#[cfg(feature = "encoding-base64")]
pub fn encode_concurrent_double_read_view_base64(histogram: &ConcurrentDoubleReadView<'_>) -> Result<String, EncodeError> {
    Ok(BASE64_STANDARD.encode(encode_concurrent_double_read_view_compressed(histogram)?))
}

#[cfg(feature = "encoding-base64")]
pub fn encode_concurrent_double_snapshot_base64<P: OverflowPolicy>(
    histogram: &ConcurrentDoubleSnapshot<'_, P>,
) -> Result<String, EncodeError> {
    Ok(BASE64_STANDARD.encode(encode_concurrent_double_snapshot_compressed(histogram)?))
}

#[cfg(feature = "encoding-base64")]
pub fn decode_double_histogram_base64(encoded: &str) -> Result<DoubleHistogram, DecodeError> {
    let compressed = BASE64_STANDARD
        .decode(encoded)
        .map_err(|err| DecodeError::Base64(err.to_string()))?;
    decode_double_histogram_compressed(&compressed)
}

#[cfg(feature = "encoding-base64")]
pub fn histogram_log_format_version_line() -> String {
    format!("#[Histogram log format version {}]\n", HISTOGRAM_LOG_FORMAT_VERSION)
}

#[cfg(feature = "encoding-base64")]
pub fn histogram_log_legend_line() -> &'static str {
    "\"StartTimestamp\",\"Interval_Length\",\"Interval_Max\",\"Interval_Compressed_Histogram\"\n"
}

#[cfg(feature = "encoding-base64")]
pub fn histogram_log_start_time_line(start_time_sec: f64) -> String {
    format!(
        "#[StartTime: {:.3} (seconds since epoch), {}]\n",
        start_time_sec,
        java_date_string(start_time_sec)
    )
}

#[cfg(feature = "encoding-base64")]
pub fn histogram_log_base_time_line(base_time_sec: f64) -> String {
    format!("#[BaseTime: {:.3} (seconds since epoch)]\n", base_time_sec)
}

#[cfg(feature = "encoding-base64")]
pub fn encode_histogram_log_line<H: EncodableHistogram>(
    histogram: &H,
    start_timestamp_sec: f64,
    end_timestamp_sec: f64,
) -> Result<String, EncodeError> {
    encode_histogram_log_line_with_max_value_unit_ratio(histogram, start_timestamp_sec, end_timestamp_sec, DEFAULT_LOG_MAX_VALUE_UNIT_RATIO)
}

#[cfg(feature = "encoding-base64")]
pub fn encode_histogram_log_line_with_max_value_unit_ratio<H: EncodableHistogram>(
    histogram: &H,
    start_timestamp_sec: f64,
    end_timestamp_sec: f64,
    max_value_unit_ratio: f64,
) -> Result<String, EncodeError> {
    let compressed = encode_histogram_compressed_with_level(histogram, 9)?;
    let payload = BASE64_STANDARD.encode(compressed);
    encode_log_line(
        histogram.meta_data().tag.as_deref(),
        start_timestamp_sec,
        end_timestamp_sec,
        histogram.get_max_value() as f64 / checked_max_value_unit_ratio(max_value_unit_ratio)?,
        &payload,
    )
}

#[cfg(feature = "encoding-base64")]
pub fn encode_double_histogram_log_line<P: OverflowPolicy>(
    histogram: &DoubleHistogram<P>,
    start_timestamp_sec: f64,
    end_timestamp_sec: f64,
) -> Result<String, EncodeError> {
    encode_double_histogram_log_line_with_max_value_unit_ratio(
        histogram,
        start_timestamp_sec,
        end_timestamp_sec,
        DEFAULT_LOG_MAX_VALUE_UNIT_RATIO,
    )
}

#[cfg(feature = "encoding-base64")]
pub fn encode_double_histogram_log_line_with_max_value_unit_ratio<P: OverflowPolicy>(
    histogram: &DoubleHistogram<P>,
    start_timestamp_sec: f64,
    end_timestamp_sec: f64,
    max_value_unit_ratio: f64,
) -> Result<String, EncodeError> {
    let compressed = encode_double_histogram_compressed_with_level(histogram, 9)?;
    let payload = BASE64_STANDARD.encode(compressed);
    encode_log_line(
        histogram.integer_histogram().meta_data.tag.as_deref(),
        start_timestamp_sec,
        end_timestamp_sec,
        histogram.get_max_value() / checked_max_value_unit_ratio(max_value_unit_ratio)?,
        &payload,
    )
}

#[cfg(feature = "encoding-base64")]
pub fn encode_concurrent_double_read_view_log_line(
    histogram: &ConcurrentDoubleReadView<'_>,
    start_timestamp_sec: f64,
    end_timestamp_sec: f64,
) -> Result<String, EncodeError> {
    encode_concurrent_double_read_view_log_line_with_max_value_unit_ratio(
        histogram,
        start_timestamp_sec,
        end_timestamp_sec,
        DEFAULT_LOG_MAX_VALUE_UNIT_RATIO,
    )
}

#[cfg(feature = "encoding-base64")]
pub fn encode_concurrent_double_read_view_log_line_with_max_value_unit_ratio(
    histogram: &ConcurrentDoubleReadView<'_>,
    start_timestamp_sec: f64,
    end_timestamp_sec: f64,
    max_value_unit_ratio: f64,
) -> Result<String, EncodeError> {
    let compressed = encode_concurrent_double_read_view_compressed_with_level(histogram, 9)?;
    let payload = BASE64_STANDARD.encode(compressed);
    encode_log_line(
        histogram.meta_data().tag.as_deref(),
        start_timestamp_sec,
        end_timestamp_sec,
        histogram.get_max_value() / checked_max_value_unit_ratio(max_value_unit_ratio)?,
        &payload,
    )
}

#[cfg(feature = "encoding-base64")]
pub fn encode_concurrent_double_snapshot_log_line<P: OverflowPolicy>(
    histogram: &ConcurrentDoubleSnapshot<'_, P>,
    start_timestamp_sec: f64,
    end_timestamp_sec: f64,
) -> Result<String, EncodeError> {
    encode_concurrent_double_snapshot_log_line_with_max_value_unit_ratio(
        histogram,
        start_timestamp_sec,
        end_timestamp_sec,
        DEFAULT_LOG_MAX_VALUE_UNIT_RATIO,
    )
}

#[cfg(feature = "encoding-base64")]
pub fn encode_concurrent_double_snapshot_log_line_with_max_value_unit_ratio<P: OverflowPolicy>(
    histogram: &ConcurrentDoubleSnapshot<'_, P>,
    start_timestamp_sec: f64,
    end_timestamp_sec: f64,
    max_value_unit_ratio: f64,
) -> Result<String, EncodeError> {
    let view = histogram.read_view();
    encode_concurrent_double_read_view_log_line_with_max_value_unit_ratio(
        &view,
        start_timestamp_sec,
        end_timestamp_sec,
        max_value_unit_ratio,
    )
}

#[cfg(feature = "encoding-base64")]
pub fn decode_histogram_log_line(line: &str) -> Result<Option<HistogramLogRecord>, DecodeError> {
    match scan_histogram_log_line(line)? {
        Some(record) => record.decode().map(Some),
        None => Ok(None),
    }
}

#[cfg(feature = "encoding-base64")]
pub fn scan_histogram_log_line(line: &str) -> Result<Option<HistogramLogScannedRecord>, DecodeError> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(None);
    }
    if line.starts_with("\"StartTimestamp\"") {
        return Ok(Some(HistogramLogScannedRecord::Legend));
    }
    if let Some(comment) = line.strip_prefix('#') {
        if let Some(start_time) = parse_log_comment_time(comment, "[StartTime:") {
            return Ok(Some(HistogramLogScannedRecord::StartTime(start_time?)));
        }
        if let Some(base_time) = parse_log_comment_time(comment, "[BaseTime:") {
            return Ok(Some(HistogramLogScannedRecord::BaseTime(base_time?)));
        }
        return Ok(Some(HistogramLogScannedRecord::Comment(comment.to_string())));
    }

    scan_histogram_log_interval_line(line).map(|entry| Some(HistogramLogScannedRecord::Interval(entry)))
}

/// Generate Java-style report strings for a small in-memory histogram log.
///
/// This is the ergonomic API for tests, diagnostics, and small logs. For large
/// input files or report outputs, use [`write_histogram_log_report`] so callers
/// can stream input from `BufRead` and stream report sections into `Write`
/// destinations.
///
/// ```ignore
/// let config = HistogramLogReportConfig {
///     csv: true,
///     output_value_unit_ratio: 1.0,
///     ..HistogramLogReportConfig::default()
/// };
/// let report = generate_histogram_log_report(log_text.lines(), &config)?;
/// ```
#[cfg(feature = "encoding-base64")]
pub fn generate_histogram_log_report<'a, I>(lines: I, config: &HistogramLogReportConfig) -> Result<HistogramLogReport, DecodeError>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut input = String::new();
    for line in lines {
        input.push_str(line);
        input.push('\n');
    }

    let mut interval_log = Vec::new();
    let mut percentile_distribution = Vec::new();
    let mut moving_window_log = Vec::new();
    let summary = write_histogram_log_report(
        Cursor::new(input),
        config,
        &mut interval_log,
        &mut percentile_distribution,
        config.moving_window.map(|_| &mut moving_window_log),
    )?;

    Ok(HistogramLogReport {
        start_time_sec: summary.start_time_sec,
        base_time_sec: summary.base_time_sec,
        processed_interval_count: summary.processed_interval_count,
        tags: summary.tags,
        interval_log: String::from_utf8(interval_log).expect("histogram report output is valid UTF-8"),
        moving_window_log: if config.moving_window.is_some() {
            Some(String::from_utf8(moving_window_log).expect("histogram report output is valid UTF-8"))
        } else {
            None
        },
        percentile_distribution: String::from_utf8(percentile_distribution).expect("histogram report output is valid UTF-8"),
    })
}

#[cfg(feature = "encoding-base64")]
pub fn generate_histogram_log_report_from_reader<R: BufRead>(
    reader: R,
    config: &HistogramLogReportConfig,
) -> Result<HistogramLogReport, DecodeError> {
    let mut interval_log = Vec::new();
    let mut percentile_distribution = Vec::new();
    let mut moving_window_log = Vec::new();
    let summary = write_histogram_log_report(
        reader,
        config,
        &mut interval_log,
        &mut percentile_distribution,
        config.moving_window.map(|_| &mut moving_window_log),
    )?;

    Ok(HistogramLogReport {
        start_time_sec: summary.start_time_sec,
        base_time_sec: summary.base_time_sec,
        processed_interval_count: summary.processed_interval_count,
        tags: summary.tags,
        interval_log: String::from_utf8(interval_log).expect("histogram report output is valid UTF-8"),
        moving_window_log: if config.moving_window.is_some() {
            Some(String::from_utf8(moving_window_log).expect("histogram report output is valid UTF-8"))
        } else {
            None
        },
        percentile_distribution: String::from_utf8(percentile_distribution).expect("histogram report output is valid UTF-8"),
    })
}

#[cfg(feature = "encoding-base64")]
pub fn write_histogram_log_report<R, WI, WP, WM>(
    reader: R,
    config: &HistogramLogReportConfig,
    interval_log: &mut WI,
    percentile_distribution: &mut WP,
    moving_window_log: Option<&mut WM>,
) -> Result<HistogramLogReportSummary, DecodeError>
where
    R: BufRead,
    WI: Write,
    WP: Write,
    WM: Write,
{
    validate_report_config(config)?;
    if config.moving_window.is_some() && moving_window_log.is_none() {
        return Err(DecodeError::InvalidLogLine(
            "moving-window report requested without a moving-window writer".to_string(),
        ));
    }

    let mut scanner = HistogramLogScanner::new(reader);
    let mut moving_window_log = moving_window_log;
    if config.processor_time_range_headers {
        interval_log.write_all(processor_time_range_header(config, "Interval percentile log").as_bytes())?;
        percentile_distribution.write_all(processor_time_range_header(config, "Overall percentile distribution").as_bytes())?;
        if let (Some(window), Some(writer)) = (config.moving_window, moving_window_log.as_deref_mut()) {
            writer.write_all(
                processor_time_range_header(config, &format!("Moving window log for {} percentile", window.percentile_to_report))
                    .as_bytes(),
            )?;
        }
    }
    if let (Some(window), Some(writer)) = (config.moving_window, moving_window_log.as_deref_mut()) {
        writer.write_all(moving_window_header(window, config.csv).as_bytes())?;
    }

    let mut tags = BTreeSet::new();
    let mut accumulator: Option<DecodedHistogram> = None;
    let mut moving_window_intervals = VecDeque::<(f64, DecodedHistogram)>::new();
    let mut processed_interval_count = 0_usize;
    let mut processor_start_time_written = false;

    while let Some(interval) = scanner.next_interval()? {
        tags.insert(interval.tag.clone());

        if interval.relative_start_time_sec < config.range_start_time_sec
            || interval.relative_start_time_sec > config.range_end_time_sec
            || !config.tag_filter.matches(interval.tag.as_deref())
        {
            continue;
        }

        let histogram = interval
            .decode_histogram()?
            .corrected_for_coordinated_omission(config.expected_interval_for_coordinated_omission_correction)?;

        if config.processor_start_time_header && !processor_start_time_written && scanner.start_time_sec() != 0.0 {
            processor_start_time_written = true;
            let start_time_line = histogram_log_start_time_line(scanner.start_time_sec());
            interval_log.write_all(start_time_line.as_bytes())?;
            percentile_distribution.write_all(start_time_line.as_bytes())?;
        }

        if accumulator.is_none() {
            accumulator = Some(histogram.empty_accumulator_like()?);
            interval_log.write_all(interval_report_header(config.csv).as_bytes())?;
        }
        let accumulator_ref = accumulator.as_mut().expect("accumulator initialized");
        accumulator_ref.add_assign(&histogram)?;
        interval_log.write_all(
            interval_report_line(
                interval.relative_end_time_sec,
                &histogram,
                accumulator_ref,
                config.output_value_unit_ratio,
                config.csv,
            )
            .as_bytes(),
        )?;

        if let (Some(window), Some(writer)) = (config.moving_window, moving_window_log.as_deref_mut()) {
            let window_cutoff_sec = interval.absolute_end_time_sec - window.length_sec;
            while moving_window_intervals
                .front()
                .map(|(end_time_sec, _)| *end_time_sec <= window_cutoff_sec)
                .unwrap_or(false)
            {
                moving_window_intervals.pop_front();
            }
            moving_window_intervals.push_back((interval.absolute_end_time_sec, histogram));
            let moving_sum = sum_histograms(moving_window_intervals.iter().map(|(_, histogram)| histogram))?;
            writer.write_all(
                moving_window_report_line(
                    interval.relative_end_time_sec,
                    &moving_sum,
                    window.percentile_to_report,
                    config.output_value_unit_ratio,
                    config.csv,
                )
                .as_bytes(),
            )?;
        }

        processed_interval_count += 1;
    }

    if let Some(accumulator) = accumulator.as_ref() {
        write_percentile_distribution_report(
            percentile_distribution,
            accumulator,
            config.percentile_ticks_per_half_distance,
            config.output_value_unit_ratio,
            config.csv,
        )?;
    }

    Ok(HistogramLogReportSummary {
        start_time_sec: scanner.start_time_sec(),
        base_time_sec: scanner.base_time_sec(),
        processed_interval_count,
        tags: tags.into_iter().collect(),
    })
}

fn encode_counts_payload<H: ReadableHistogram>(histogram: &H, encoded: &mut Vec<u8>) -> Result<(), EncodeError> {
    let max_value_index = histogram.settings().counts_array_index(histogram.get_max_value());
    let counts_limit = max_value_index.saturating_add(1).min(histogram.array_length());
    let mut src_index = 0;

    while src_index < counts_limit {
        let count = histogram.unsafe_get_count_at_index(src_index);
        if count > i64::MAX as u64 {
            return Err(EncodeError::CountTooLarge(count));
        }
        src_index += 1;

        let mut zeros_count = 0_i64;
        if count == 0 {
            zeros_count = 1;
            while src_index < counts_limit && histogram.unsafe_get_count_at_index(src_index) == 0 {
                zeros_count += 1;
                src_index += 1;
            }
        }

        if zeros_count > 1 {
            write_zig_zag_i64(encoded, -zeros_count);
        } else {
            write_zig_zag_i64(encoded, count as i64);
        }
    }

    Ok(())
}

#[cfg(feature = "encoding-base64")]
fn encode_log_line(
    tag: Option<&str>,
    start_timestamp_sec: f64,
    end_timestamp_sec: f64,
    max_value: f64,
    payload: &str,
) -> Result<String, EncodeError> {
    if !start_timestamp_sec.is_finite() || !end_timestamp_sec.is_finite() || end_timestamp_sec < start_timestamp_sec {
        return Err(EncodeError::InvalidLogLine("invalid histogram log interval timestamps".to_string()));
    }
    if !max_value.is_finite() {
        return Err(EncodeError::InvalidLogLine("invalid histogram log max value".to_string()));
    }

    let interval_length_sec = end_timestamp_sec - start_timestamp_sec;
    match tag {
        Some(tag) => {
            if tag.is_empty() || tag.contains(',') || tag.chars().any(char::is_whitespace) {
                return Err(EncodeError::InvalidLogLine(
                    "tag string cannot be empty or contain commas or whitespace".to_string(),
                ));
            }
            Ok(format!(
                "Tag={},{:.3},{:.3},{:.3},{}\n",
                tag, start_timestamp_sec, interval_length_sec, max_value, payload
            ))
        }
        None => Ok(format!(
            "{:.3},{:.3},{:.3},{}\n",
            start_timestamp_sec, interval_length_sec, max_value, payload
        )),
    }
}

#[cfg(feature = "encoding-base64")]
fn checked_max_value_unit_ratio(max_value_unit_ratio: f64) -> Result<f64, EncodeError> {
    if !max_value_unit_ratio.is_finite() || max_value_unit_ratio <= 0.0 {
        return Err(EncodeError::InvalidLogLine(
            "max value unit ratio must be finite and positive".to_string(),
        ));
    }
    Ok(max_value_unit_ratio)
}

#[cfg(feature = "encoding-base64")]
fn scan_histogram_log_interval_line(line: &str) -> Result<HistogramLogScannedInterval, DecodeError> {
    let (tag, fields) = split_log_tag_and_fields(line)?;
    let mut fields = fields
        .split(|ch: char| ch == ',' || ch.is_whitespace())
        .filter(|field| !field.is_empty());
    let start_timestamp_sec = parse_log_f64(fields.next(), line)?;
    let interval_length_sec = parse_log_f64(fields.next(), line)?;
    let max_value = parse_log_f64(fields.next(), line)?;
    if !start_timestamp_sec.is_finite() || !interval_length_sec.is_finite() || interval_length_sec < 0.0 || !max_value.is_finite() {
        return Err(DecodeError::InvalidLogLine(line.to_string()));
    }
    let payload = fields.next().ok_or_else(|| DecodeError::InvalidLogLine(line.to_string()))?;
    if fields.next().is_some() {
        return Err(DecodeError::InvalidLogLine(line.to_string()));
    }

    Ok(HistogramLogScannedInterval {
        tag,
        start_timestamp_sec,
        interval_length_sec,
        max_value,
        absolute_start_time_sec: 0.0,
        absolute_end_time_sec: 0.0,
        relative_start_time_sec: 0.0,
        relative_end_time_sec: 0.0,
        compressed_histogram_base64: payload.to_string(),
    })
}

#[cfg(feature = "encoding-base64")]
fn split_log_tag_and_fields(line: &str) -> Result<(Option<String>, &str), DecodeError> {
    if let Some(rest) = line.strip_prefix("Tag=") {
        let delimiter_index = rest
            .find(|ch: char| ch == ',' || ch.is_whitespace())
            .ok_or_else(|| DecodeError::InvalidLogLine(line.to_string()))?;
        let (tag, fields) = rest.split_at(delimiter_index);
        if tag.is_empty() {
            return Err(DecodeError::InvalidLogLine(line.to_string()));
        }
        let fields = fields.trim_start_matches(|ch: char| ch == ',' || ch.is_whitespace());
        return Ok((Some(tag.to_string()), fields));
    }
    Ok((None, line))
}

#[cfg(feature = "encoding-base64")]
fn decode_histogram_log_payload(payload: &str) -> Result<DecodedHistogram, DecodeError> {
    let compressed = BASE64_STANDARD
        .decode(payload)
        .map_err(|err| DecodeError::Base64(err.to_string()))?;
    decode(&compressed)
}

#[cfg(feature = "encoding-base64")]
fn decoded_interval_from_scanned(interval: HistogramLogScannedInterval) -> Result<HistogramLogInterval, DecodeError> {
    let histogram = interval.decode_histogram()?;
    Ok(HistogramLogInterval {
        tag: interval.tag,
        start_timestamp_sec: interval.start_timestamp_sec,
        interval_length_sec: interval.interval_length_sec,
        max_value: interval.max_value,
        absolute_start_time_sec: interval.absolute_start_time_sec,
        absolute_end_time_sec: interval.absolute_end_time_sec,
        relative_start_time_sec: interval.relative_start_time_sec,
        relative_end_time_sec: interval.relative_end_time_sec,
        histogram,
    })
}

#[cfg(feature = "encoding-base64")]
fn parse_log_comment_time(comment: &str, prefix: &str) -> Option<Result<f64, DecodeError>> {
    let rest = comment.strip_prefix(prefix)?.trim_start();
    let value = rest
        .split(|ch: char| ch == ',' || ch == ']' || ch.is_whitespace())
        .find(|part| !part.is_empty())
        .ok_or_else(|| DecodeError::InvalidLogLine(comment.to_string()));
    Some(value.and_then(|value| value.parse::<f64>().map_err(|_| DecodeError::InvalidLogLine(comment.to_string()))))
}

#[cfg(feature = "encoding-base64")]
fn parse_log_f64(field: Option<&str>, line: &str) -> Result<f64, DecodeError> {
    let field = field.ok_or_else(|| DecodeError::InvalidLogLine(line.to_string()))?;
    field.parse::<f64>().map_err(|_| DecodeError::InvalidLogLine(line.to_string()))
}

#[cfg(feature = "encoding-base64")]
impl HistogramLogTagFilter {
    fn matches(&self, tag: Option<&str>) -> bool {
        match self {
            HistogramLogTagFilter::Untagged => tag.is_none(),
            HistogramLogTagFilter::Tag(expected) => tag == Some(expected.as_str()),
            HistogramLogTagFilter::Any => true,
        }
    }
}

#[cfg(feature = "encoding-base64")]
impl DecodedHistogram {
    fn corrected_for_coordinated_omission(self, expected_interval: f64) -> Result<Self, DecodeError> {
        if expected_interval == 0.0 {
            return Ok(self);
        }

        match self {
            DecodedHistogram::Integer(histogram) => {
                if expected_interval > u64::MAX as f64 {
                    return Err(DecodeError::InvalidLogLine(
                        "integer coordinated-omission interval is too large".to_string(),
                    ));
                }
                let expected_interval = expected_interval as u64;
                if expected_interval == 0 {
                    return Ok(DecodedHistogram::Integer(histogram));
                }
                let mut target = empty_integer_accumulator_like(&histogram)?;
                for value in histogram.recorded_values() {
                    target
                        .record_value_with_count_and_expected_interval(
                            value.value_iterated_to,
                            value.count_at_value_iterated_to,
                            expected_interval,
                        )
                        .map_err(|err| report_record_error("failed to correct integer histogram", err))?;
                }
                Ok(DecodedHistogram::Integer(target))
            }
            DecodedHistogram::Double(histogram) => histogram
                .copy_corrected_for_coordinated_omission(expected_interval)
                .map(DecodedHistogram::Double)
                .map_err(|err| report_record_error("failed to correct double histogram", err)),
        }
    }

    fn empty_accumulator_like(&self) -> Result<Self, DecodeError> {
        match self {
            DecodedHistogram::Integer(histogram) => empty_integer_accumulator_like(histogram).map(DecodedHistogram::Integer),
            DecodedHistogram::Double(histogram) => empty_double_accumulator_like(histogram).map(DecodedHistogram::Double),
        }
    }

    fn add_assign(&mut self, other: &Self) -> Result<(), DecodeError> {
        match (self, other) {
            (DecodedHistogram::Integer(target), DecodedHistogram::Integer(source)) => target
                .add(source)
                .map_err(|err| report_record_error("failed to add integer histogram", err)),
            (DecodedHistogram::Double(target), DecodedHistogram::Double(source)) => target
                .add(source)
                .map_err(|err| report_record_error("failed to add double histogram", err)),
            _ => Err(DecodeError::InvalidLogLine(
                "histogram log mixes integer and double histograms".to_string(),
            )),
        }
    }

    fn get_total_count(&self) -> u64 {
        match self {
            DecodedHistogram::Integer(histogram) => histogram.get_total_count(),
            DecodedHistogram::Double(histogram) => histogram.get_total_count(),
        }
    }

    fn get_value_at_percentile(&self, percentile: f64) -> f64 {
        match self {
            DecodedHistogram::Integer(histogram) => histogram.get_value_at_percentile(percentile) as f64,
            DecodedHistogram::Double(histogram) => histogram.get_value_at_percentile(percentile),
        }
    }

    fn get_max_value(&self) -> f64 {
        match self {
            DecodedHistogram::Integer(histogram) => histogram.get_max_value() as f64,
            DecodedHistogram::Double(histogram) => histogram.get_max_value(),
        }
    }

    fn get_mean(&self) -> f64 {
        match self {
            DecodedHistogram::Integer(histogram) => histogram.get_mean(),
            DecodedHistogram::Double(histogram) => histogram.get_mean(),
        }
    }

    fn get_std_deviation(&self) -> f64 {
        match self {
            DecodedHistogram::Integer(histogram) => histogram.get_std_deviation(),
            DecodedHistogram::Double(histogram) => histogram.get_std_deviation(),
        }
    }

    fn number_of_significant_value_digits(&self) -> usize {
        match self {
            DecodedHistogram::Integer(histogram) => histogram.get_number_of_significant_value_digits() as usize,
            DecodedHistogram::Double(histogram) => histogram.get_number_of_significant_value_digits() as usize,
        }
    }

    fn bucket_count(&self) -> u32 {
        match self {
            DecodedHistogram::Integer(histogram) => histogram.settings().bucket_count,
            DecodedHistogram::Double(histogram) => histogram.integer_histogram().settings().bucket_count,
        }
    }

    fn sub_bucket_count(&self) -> u32 {
        match self {
            DecodedHistogram::Integer(histogram) => histogram.settings().sub_bucket_count,
            DecodedHistogram::Double(histogram) => histogram.integer_histogram().settings().sub_bucket_count,
        }
    }
}

#[cfg(feature = "encoding-base64")]
fn validate_report_config(config: &HistogramLogReportConfig) -> Result<(), DecodeError> {
    validate_time_range(config.range_start_time_sec, config.range_end_time_sec)?;
    if !config.output_value_unit_ratio.is_finite() || config.output_value_unit_ratio <= 0.0 {
        return Err(DecodeError::InvalidLogLine(
            "output value unit ratio must be finite and positive".to_string(),
        ));
    }
    if config.percentile_ticks_per_half_distance == 0 {
        return Err(DecodeError::InvalidLogLine(
            "percentile ticks per half distance must be greater than zero".to_string(),
        ));
    }
    if !config.expected_interval_for_coordinated_omission_correction.is_finite()
        || config.expected_interval_for_coordinated_omission_correction < 0.0
    {
        return Err(DecodeError::InvalidLogLine(
            "coordinated-omission interval must be finite and non-negative".to_string(),
        ));
    }
    if let Some(window) = config.moving_window {
        if !window.length_sec.is_finite() || window.length_sec <= 0.0 {
            return Err(DecodeError::InvalidLogLine(
                "moving-window length must be finite and positive".to_string(),
            ));
        }
        if !window.percentile_to_report.is_finite() || window.percentile_to_report < 0.0 || window.percentile_to_report > 100.0 {
            return Err(DecodeError::InvalidLogLine(
                "moving-window percentile must be between 0 and 100".to_string(),
            ));
        }
    }
    Ok(())
}

#[cfg(feature = "encoding-base64")]
fn validate_time_range(range_start_time_sec: f64, range_end_time_sec: f64) -> Result<(), DecodeError> {
    if !range_start_time_sec.is_finite() || range_end_time_sec.is_nan() {
        return Err(DecodeError::InvalidLogLine(
            "range start must be finite and range end must not be NaN".to_string(),
        ));
    }
    if range_end_time_sec < range_start_time_sec {
        return Err(DecodeError::InvalidLogLine("range end must not be before range start".to_string()));
    }
    Ok(())
}

#[cfg(feature = "encoding-base64")]
fn processor_time_range_header(config: &HistogramLogReportConfig, title: &str) -> String {
    let end_time = if config.range_end_time_sec < f64::MAX {
        format!("{:.3}", config.range_end_time_sec)
    } else {
        "<Infinite>".to_string()
    };
    format!(
        "#[{} between {:.3} and {} seconds (relative to StartTime)]\n",
        title, config.range_start_time_sec, end_time
    )
}

#[cfg(feature = "encoding-base64")]
fn java_date_string(start_time_sec: f64) -> String {
    let millis = start_time_sec * 1000.0;
    if !millis.is_finite() || millis < i64::MIN as f64 || millis > i64::MAX as f64 {
        return "Invalid Date".to_string();
    }
    match Local.timestamp_millis_opt(millis as i64).single() {
        Some(time) => time.format("%a %b %d %H:%M:%S %Z %Y").to_string(),
        None => "Invalid Date".to_string(),
    }
}

#[cfg(feature = "encoding-base64")]
fn empty_integer_accumulator_like(histogram: &Histogram) -> Result<Histogram, DecodeError> {
    let mut target = Histogram::with_low_high_sigvdig(
        histogram.get_lowest_discernible_value(),
        histogram.get_highest_trackable_value(),
        histogram.get_number_of_significant_value_digits() as u8,
    )?;
    target.set_auto_resize(true);
    target.set_integer_to_double_value_conversion_ratio(histogram.integer_to_double_value_conversion_ratio());
    target.set_normalizing_index_offset(histogram.normalizing_index_offset());
    Ok(target)
}

#[cfg(feature = "encoding-base64")]
fn empty_double_accumulator_like(histogram: &DoubleHistogram) -> Result<DoubleHistogram, DecodeError> {
    let integer_histogram = histogram.integer_histogram();
    let integer_accumulator = empty_integer_accumulator_like(integer_histogram)?;
    let mut target = DoubleHistogram::from_integer_histogram(
        histogram.get_highest_to_lowest_value_ratio(),
        histogram.get_number_of_significant_value_digits(),
        integer_accumulator,
    )?;
    target.set_auto_resize(true);
    Ok(target)
}

#[cfg(feature = "encoding-base64")]
fn sum_histograms<'a, I>(histograms: I) -> Result<DecodedHistogram, DecodeError>
where
    I: IntoIterator<Item = &'a DecodedHistogram>,
{
    let mut iter = histograms.into_iter();
    let first = iter
        .next()
        .ok_or_else(|| DecodeError::InvalidLogLine("moving-window sum is empty".to_string()))?;
    let mut sum = first.empty_accumulator_like()?;
    sum.add_assign(first)?;
    for histogram in iter {
        sum.add_assign(histogram)?;
    }
    Ok(sum)
}

#[cfg(feature = "encoding-base64")]
fn interval_report_header(csv: bool) -> &'static str {
    if csv {
        "\"Timestamp\",\"Int_Count\",\"Int_50%\",\"Int_90%\",\"Int_Max\",\"Total_Count\",\"Total_50%\",\"Total_90%\",\"Total_99%\",\"Total_99.9%\",\"Total_99.99%\",\"Total_Max\"\n"
    } else {
        "Time: IntervalPercentiles:count ( 50% 90% Max ) TotalPercentiles:count ( 50% 90% 99% 99.9% 99.99% Max )\n"
    }
}

#[cfg(feature = "encoding-base64")]
fn interval_report_line(
    timestamp_sec: f64,
    interval: &DecodedHistogram,
    total: &DecodedHistogram,
    output_value_unit_ratio: f64,
    csv: bool,
) -> String {
    if csv {
        format!(
            "{:.3},{},{:.3},{:.3},{:.3},{},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3}\n",
            timestamp_sec,
            interval.get_total_count(),
            interval.get_value_at_percentile(50.0) / output_value_unit_ratio,
            interval.get_value_at_percentile(90.0) / output_value_unit_ratio,
            interval.get_max_value() / output_value_unit_ratio,
            total.get_total_count(),
            total.get_value_at_percentile(50.0) / output_value_unit_ratio,
            total.get_value_at_percentile(90.0) / output_value_unit_ratio,
            total.get_value_at_percentile(99.0) / output_value_unit_ratio,
            total.get_value_at_percentile(99.9) / output_value_unit_ratio,
            total.get_value_at_percentile(99.99) / output_value_unit_ratio,
            total.get_max_value() / output_value_unit_ratio,
        )
    } else {
        format!(
            "{:4.3}: I:{} ( {:7.3} {:7.3} {:7.3} ) T:{} ( {:7.3} {:7.3} {:7.3} {:7.3} {:7.3} {:7.3} )\n",
            timestamp_sec,
            interval.get_total_count(),
            interval.get_value_at_percentile(50.0) / output_value_unit_ratio,
            interval.get_value_at_percentile(90.0) / output_value_unit_ratio,
            interval.get_max_value() / output_value_unit_ratio,
            total.get_total_count(),
            total.get_value_at_percentile(50.0) / output_value_unit_ratio,
            total.get_value_at_percentile(90.0) / output_value_unit_ratio,
            total.get_value_at_percentile(99.0) / output_value_unit_ratio,
            total.get_value_at_percentile(99.9) / output_value_unit_ratio,
            total.get_value_at_percentile(99.99) / output_value_unit_ratio,
            total.get_max_value() / output_value_unit_ratio,
        )
    }
}

#[cfg(feature = "encoding-base64")]
fn moving_window_header(window: HistogramLogMovingWindowConfig, csv: bool) -> String {
    if csv {
        format!("\"Timestamp\",\"Window_Count\",\"{}%'ile\",\"Max\"\n", window.percentile_to_report)
    } else {
        format!("Time: WindowCount {}%'ile Max\n", window.percentile_to_report)
    }
}

#[cfg(feature = "encoding-base64")]
fn moving_window_report_line(
    timestamp_sec: f64,
    moving_sum: &DecodedHistogram,
    percentile_to_report: f64,
    output_value_unit_ratio: f64,
    csv: bool,
) -> String {
    if csv {
        format!(
            "{:.3},{},{:.3},{:.3}\n",
            timestamp_sec,
            moving_sum.get_total_count(),
            moving_sum.get_value_at_percentile(percentile_to_report) / output_value_unit_ratio,
            moving_sum.get_max_value() / output_value_unit_ratio,
        )
    } else {
        format!(
            "{:4.3}: I:{} P:{:7.3} M:{:7.3}\n",
            timestamp_sec,
            moving_sum.get_total_count(),
            moving_sum.get_value_at_percentile(percentile_to_report) / output_value_unit_ratio,
            moving_sum.get_max_value() / output_value_unit_ratio,
        )
    }
}

#[cfg(feature = "encoding-base64")]
fn percentile_distribution_report(
    histogram: &DecodedHistogram,
    percentile_ticks_per_half_distance: u32,
    output_value_unit_ratio: f64,
    csv: bool,
) -> String {
    let mut output = Vec::new();
    write_percentile_distribution_report(
        &mut output,
        histogram,
        percentile_ticks_per_half_distance,
        output_value_unit_ratio,
        csv,
    )
    .expect("writing to Vec cannot fail");
    String::from_utf8(output).expect("histogram report output is valid UTF-8")
}

#[cfg(feature = "encoding-base64")]
fn write_percentile_distribution_report<W: Write>(
    output: &mut W,
    histogram: &DecodedHistogram,
    percentile_ticks_per_half_distance: u32,
    output_value_unit_ratio: f64,
    csv: bool,
) -> Result<(), DecodeError> {
    if csv {
        output.write_all(b"\"Value\",\"Percentile\",\"TotalCount\",\"1/(1-Percentile)\"\n")?;
    } else {
        output.write_all(
            format!(
                "{:>12} {:>14} {:>10} {:>14}\n\n",
                "Value", "Percentile", "TotalCount", "1/(1-Percentile)"
            )
            .as_bytes(),
        )?;
    }

    let precision = histogram.number_of_significant_value_digits();
    match histogram {
        DecodedHistogram::Integer(histogram) => {
            for value in histogram.percentiles(percentile_ticks_per_half_distance) {
                write_percentile_distribution_line(
                    output,
                    value.value_iterated_to as f64 / output_value_unit_ratio,
                    value.percentile_level_iterated_to / 100.0,
                    value.total_count_to_this_value,
                    precision,
                    csv,
                )?;
            }
        }
        DecodedHistogram::Double(histogram) => {
            let ratio = histogram.integer_histogram().integer_to_double_value_conversion_ratio();
            for value in histogram.integer_histogram().percentiles(percentile_ticks_per_half_distance) {
                write_percentile_distribution_line(
                    output,
                    value.value_iterated_to as f64 * ratio / output_value_unit_ratio,
                    value.percentile_level_iterated_to / 100.0,
                    value.total_count_to_this_value,
                    precision,
                    csv,
                )?;
            }
        }
    }

    if !csv {
        output.write_all(
            format!(
                "#[Mean    = {:12.*}, StdDeviation   = {:12.*}]\n",
                precision,
                histogram.get_mean() / output_value_unit_ratio,
                precision,
                histogram.get_std_deviation() / output_value_unit_ratio,
            )
            .as_bytes(),
        )?;
        output.write_all(
            format!(
                "#[Max     = {:12.*}, Total count    = {:12}]\n",
                precision,
                histogram.get_max_value() / output_value_unit_ratio,
                histogram.get_total_count(),
            )
            .as_bytes(),
        )?;
        output.write_all(
            format!(
                "#[Buckets = {:12}, SubBuckets     = {:12}]\n",
                histogram.bucket_count(),
                histogram.sub_bucket_count(),
            )
            .as_bytes(),
        )?;
    }

    Ok(())
}

#[cfg(feature = "encoding-base64")]
fn write_percentile_distribution_line<W: Write>(
    output: &mut W,
    value: f64,
    percentile: f64,
    total_count: u64,
    precision: usize,
    csv: bool,
) -> Result<(), DecodeError> {
    if csv {
        if percentile == 1.0 {
            output.write_all(format!("{:.*},{:.12},{},Infinity\n", precision, value, percentile, total_count).as_bytes())?;
        } else {
            output.write_all(
                format!(
                    "{:.*},{:.12},{},{:.2}\n",
                    precision,
                    value,
                    percentile,
                    total_count,
                    1.0 / (1.0 - percentile),
                )
                .as_bytes(),
            )?;
        }
    } else if percentile == 1.0 {
        output.write_all(format!("{:12.*} {:2.12} {:10}\n", precision, value, percentile, total_count).as_bytes())?;
    } else {
        output.write_all(
            format!(
                "{:12.*} {:2.12} {:10} {:14.2}\n",
                precision,
                value,
                percentile,
                total_count,
                1.0 / (1.0 - percentile),
            )
            .as_bytes(),
        )?;
    }
    Ok(())
}

#[cfg(feature = "encoding-base64")]
fn report_record_error(context: &str, err: impl std::fmt::Debug) -> DecodeError {
    DecodeError::InvalidLogLine(format!("{}: {:?}", context, err))
}

fn decode_counts_payload(payload: &[u8], word_size: u8, histogram: &mut Histogram) -> Result<(), DecodeError> {
    let mut reader = Reader::new(payload);
    let mut dst_index = 0_u32;
    let counts_array_length = histogram.counts_array_length();
    let mut observed_total_count = 0_u64;

    while !reader.is_empty() {
        let count = if word_size == V2_MAX_WORD_SIZE_IN_BYTES {
            let encoded_count = reader.read_zig_zag_i64()?;
            if encoded_count < 0 {
                let zeros_count = encoded_count.checked_neg().ok_or(DecodeError::InvalidPayload)?;
                if zeros_count > i32::MAX as i64 {
                    return Err(DecodeError::InvalidPayload);
                }
                dst_index = dst_index.checked_add(zeros_count as u32).ok_or(DecodeError::InvalidPayload)?;
                if dst_index > counts_array_length {
                    return Err(DecodeError::CountsArrayIndexOutOfBounds(dst_index));
                }
                continue;
            }
            encoded_count as u64
        } else {
            reader.read_fixed_count(word_size)?
        };

        if dst_index >= counts_array_length {
            return Err(DecodeError::CountsArrayIndexOutOfBounds(dst_index));
        }
        histogram.set_count_at_logical_index(dst_index, count);
        observed_total_count = observed_total_count.checked_add(count).ok_or(DecodeError::CountOverflow)?;
        dst_index = dst_index.checked_add(1).ok_or(DecodeError::InvalidPayload)?;
    }

    Ok(())
}

#[cfg(feature = "encoding-compression")]
fn encode_compressed_payload(cookie: u32, raw: &[u8], compression: Compression) -> Result<Vec<u8>, EncodeError> {
    let compressed = compress_zlib(raw, compression)?;
    if compressed.len() > i32::MAX as usize {
        return Err(EncodeError::PayloadTooLarge(compressed.len()));
    }

    let mut encoded = Vec::with_capacity(8 + compressed.len());
    write_u32(&mut encoded, cookie);
    write_i32(&mut encoded, compressed.len() as i32);
    encoded.extend_from_slice(&compressed);
    Ok(encoded)
}

#[cfg(feature = "encoding-compression")]
fn compress_zlib(raw: &[u8], compression: Compression) -> Result<Vec<u8>, EncodeError> {
    let mut encoder = ZlibEncoder::new(raw, compression);
    let mut compressed = Vec::new();
    encoder
        .read_to_end(&mut compressed)
        .map_err(|err| EncodeError::Compression(err.to_string()))?;
    Ok(compressed)
}

#[cfg(feature = "encoding-compression")]
fn decompress_histogram_raw(compressed_cookie: u32, compressed: &[u8], min_highest_trackable_value: u64) -> Result<Vec<u8>, DecodeError> {
    let compressed_base = cookie_base(compressed_cookie);
    let header_size = match compressed_base {
        V2_COMPRESSED_ENCODING_COOKIE_BASE | V1_COMPRESSED_ENCODING_COOKIE_BASE => ENCODING_HEADER_SIZE,
        V0_COMPRESSED_ENCODING_COOKIE_BASE => V0_ENCODING_HEADER_SIZE,
        _ => return Err(DecodeError::InvalidCookie(compressed_cookie)),
    };

    let mut decoder = ZlibDecoder::new(compressed);
    let mut raw = vec![0; header_size];
    decoder.read_exact(&mut raw).map_err(decode_compression_read_error)?;

    let payload_limit = compressed_payload_limit_from_raw_header(&raw, min_highest_trackable_value)?;
    if let Some(payload_length) = payload_limit.exact_length {
        raw.try_reserve_exact(payload_length).map_err(|_| DecodeError::InvalidPayload)?;
        let payload_start = raw.len();
        raw.resize(payload_start + payload_length, 0);
        decoder
            .read_exact(&mut raw[payload_start..])
            .map_err(decode_compression_read_error)?;
    } else {
        let max_payload_length = payload_limit.max_length;
        let read_limit = max_payload_length.checked_add(1).ok_or(DecodeError::InvalidPayload)?;
        decoder
            .take(read_limit as u64)
            .read_to_end(&mut raw)
            .map_err(decode_compression_read_error)?;
        if raw.len() > header_size + max_payload_length {
            return Err(DecodeError::InvalidPayload);
        }
    }

    Ok(raw)
}

#[cfg(feature = "encoding-compression")]
struct CompressedPayloadLimit {
    exact_length: Option<usize>,
    max_length: usize,
}

#[cfg(feature = "encoding-compression")]
fn compressed_payload_limit_from_raw_header(
    raw_header: &[u8],
    min_highest_trackable_value: u64,
) -> Result<CompressedPayloadLimit, DecodeError> {
    let mut reader = Reader::new(raw_header);
    let cookie = reader.read_u32()?;
    let base = cookie_base(cookie);
    let word_size = word_size_from_cookie(cookie);

    let (payload_length, number_of_significant_value_digits, lowest_discernible_value, highest_trackable_value) = match base {
        V2_ENCODING_COOKIE_BASE | V1_ENCODING_COOKIE_BASE => {
            if base == V2_ENCODING_COOKIE_BASE && word_size != V2_MAX_WORD_SIZE_IN_BYTES {
                return Err(DecodeError::UnsupportedWordSize(word_size));
            }
            if base == V1_ENCODING_COOKIE_BASE && !matches!(word_size, 2 | 4 | 8) {
                return Err(DecodeError::UnsupportedWordSize(word_size));
            }

            let payload_length = reader.read_i32()?;
            if payload_length < 0 {
                return Err(DecodeError::InvalidPayloadLength(payload_length));
            }
            let _normalizing_index_offset = reader.read_i32()?;
            let number_of_significant_value_digits = reader.read_i32()?;
            let lowest_discernible_value = reader.read_non_negative_i64()?;
            let highest_trackable_value = reader.read_non_negative_i64()?;
            let integer_to_double_value_conversion_ratio = reader.read_f64()?;
            if !integer_to_double_value_conversion_ratio.is_finite() || integer_to_double_value_conversion_ratio <= 0.0 {
                return Err(DecodeError::InvalidPayload);
            }

            (
                Some(payload_length as usize),
                number_of_significant_value_digits,
                lowest_discernible_value,
                highest_trackable_value,
            )
        }
        V0_ENCODING_COOKIE_BASE => {
            if !matches!(word_size, 2 | 4 | 8) {
                return Err(DecodeError::UnsupportedWordSize(word_size));
            }
            let number_of_significant_value_digits = reader.read_i32()?;
            let lowest_discernible_value = reader.read_non_negative_i64()?;
            let highest_trackable_value = reader.read_non_negative_i64()?;
            let _total_count = reader.read_i64()?;

            (
                None,
                number_of_significant_value_digits,
                lowest_discernible_value,
                highest_trackable_value,
            )
        }
        _ => return Err(DecodeError::InvalidCookie(cookie)),
    };

    if number_of_significant_value_digits < 0 || number_of_significant_value_digits > u8::MAX as i32 {
        return Err(DecodeError::InvalidPayload);
    }

    let settings = HistogramSettings::new(
        lowest_discernible_value,
        highest_trackable_value.max(min_highest_trackable_value),
        number_of_significant_value_digits as u8,
    )?;
    let max_length = (settings.counts_array_length as usize)
        .checked_mul(word_size as usize)
        .ok_or(DecodeError::InvalidPayload)?;
    if let Some(payload_length) = payload_length {
        if payload_length > max_length {
            return Err(DecodeError::InvalidPayload);
        }
    }

    Ok(CompressedPayloadLimit {
        exact_length: payload_length,
        max_length,
    })
}

#[cfg(feature = "encoding-compression")]
fn decode_compression_read_error(err: std::io::Error) -> DecodeError {
    if err.kind() == ErrorKind::UnexpectedEof {
        DecodeError::UnexpectedEof
    } else {
        DecodeError::Compression(err.to_string())
    }
}

fn read_significant_value_digits(reader: &mut Reader<'_>) -> Result<u8, DecodeError> {
    let number_of_significant_value_digits = reader.read_i32()?;
    if number_of_significant_value_digits < 0 || number_of_significant_value_digits > u8::MAX as i32 {
        return Err(DecodeError::InvalidPayload);
    }
    Ok(number_of_significant_value_digits as u8)
}

fn is_histogram_encoding_cookie(cookie: u32) -> bool {
    matches!(
        cookie_base(cookie),
        V2_ENCODING_COOKIE_BASE | V1_ENCODING_COOKIE_BASE | V0_ENCODING_COOKIE_BASE
    )
}

fn is_histogram_compressed_encoding_cookie(cookie: u32) -> bool {
    matches!(
        cookie_base(cookie),
        V2_COMPRESSED_ENCODING_COOKIE_BASE | V1_COMPRESSED_ENCODING_COOKIE_BASE | V0_COMPRESSED_ENCODING_COOKIE_BASE
    )
}

fn cookie_base(cookie: u32) -> u32 {
    cookie & !0xf0
}

fn word_size_from_cookie(cookie: u32) -> u8 {
    if matches!(cookie_base(cookie), V2_ENCODING_COOKIE_BASE | V2_COMPRESSED_ENCODING_COOKIE_BASE) {
        return V2_MAX_WORD_SIZE_IN_BYTES;
    }
    (((cookie & 0xf0) >> 4) as u8) & 0xe
}

fn peek_cookie(bytes: &[u8]) -> Result<u32, DecodeError> {
    Reader::new(bytes).read_u32()
}

fn write_u32(encoded: &mut Vec<u8>, value: u32) {
    encoded.extend_from_slice(&value.to_be_bytes());
}

fn write_i32(encoded: &mut Vec<u8>, value: i32) {
    encoded.extend_from_slice(&value.to_be_bytes());
}

fn write_non_negative_i64(encoded: &mut Vec<u8>, value: u64) -> Result<(), EncodeError> {
    if value > i64::MAX as u64 {
        return Err(EncodeError::ValueTooLarge(value));
    }
    encoded.extend_from_slice(&(value as i64).to_be_bytes());
    Ok(())
}

fn write_f64(encoded: &mut Vec<u8>, value: f64) {
    encoded.extend_from_slice(&value.to_be_bytes());
}

fn write_zig_zag_i64(encoded: &mut Vec<u8>, value: i64) {
    let mut shifted = ((value as u64) << 1) ^ ((value >> 63) as u64);
    for _ in 0..8 {
        if shifted >> 7 == 0 {
            encoded.push(shifted as u8);
            return;
        }
        encoded.push(((shifted & 0x7f) | 0x80) as u8);
        shifted >>= 7;
    }
    encoded.push(shifted as u8);
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Reader { bytes, position: 0 }
    }

    fn is_empty(&self) -> bool {
        self.position == self.bytes.len()
    }

    fn remaining(&self) -> &'a [u8] {
        &self.bytes[self.position..]
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], DecodeError> {
        let end = self.position.checked_add(length).ok_or(DecodeError::InvalidPayload)?;
        if end > self.bytes.len() {
            return Err(DecodeError::UnexpectedEof);
        }
        let slice = &self.bytes[self.position..end];
        self.position = end;
        Ok(slice)
    }

    fn read_u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }

    fn read_u32(&mut self) -> Result<u32, DecodeError> {
        let mut bytes = [0; 4];
        bytes.copy_from_slice(self.take(4)?);
        Ok(u32::from_be_bytes(bytes))
    }

    fn read_i32(&mut self) -> Result<i32, DecodeError> {
        let mut bytes = [0; 4];
        bytes.copy_from_slice(self.take(4)?);
        Ok(i32::from_be_bytes(bytes))
    }

    fn read_i64(&mut self) -> Result<i64, DecodeError> {
        let mut bytes = [0; 8];
        bytes.copy_from_slice(self.take(8)?);
        Ok(i64::from_be_bytes(bytes))
    }

    fn read_non_negative_i64(&mut self) -> Result<u64, DecodeError> {
        let value = self.read_i64()?;
        if value < 0 {
            return Err(DecodeError::InvalidPayload);
        }
        Ok(value as u64)
    }

    fn read_f64(&mut self) -> Result<f64, DecodeError> {
        let mut bytes = [0; 8];
        bytes.copy_from_slice(self.take(8)?);
        Ok(f64::from_be_bytes(bytes))
    }

    fn read_fixed_count(&mut self, word_size: u8) -> Result<u64, DecodeError> {
        match word_size {
            2 => {
                let mut bytes = [0; 2];
                bytes.copy_from_slice(self.take(2)?);
                let count = i16::from_be_bytes(bytes);
                if count < 0 {
                    Err(DecodeError::InvalidPayload)
                } else {
                    Ok(count as u64)
                }
            }
            4 => {
                let count = self.read_i32()?;
                if count < 0 {
                    Err(DecodeError::InvalidPayload)
                } else {
                    Ok(count as u64)
                }
            }
            8 => {
                let count = self.read_i64()?;
                if count < 0 {
                    Err(DecodeError::InvalidPayload)
                } else {
                    Ok(count as u64)
                }
            }
            _ => Err(DecodeError::UnsupportedWordSize(word_size)),
        }
    }

    fn read_zig_zag_i64(&mut self) -> Result<i64, DecodeError> {
        let mut raw = 0_u64;
        for index in 0..9 {
            let byte = self.read_u8()? as u64;
            if index == 8 {
                raw |= byte << 56;
                return Ok(decode_zig_zag_i64(raw));
            }

            raw |= (byte & 0x7f) << (7 * index);
            if (byte & 0x80) == 0 {
                return Ok(decode_zig_zag_i64(raw));
            }
        }

        Err(DecodeError::InvalidPayload)
    }
}

fn decode_zig_zag_i64(raw: u64) -> i64 {
    ((raw >> 1) as i64) ^ (-((raw & 1) as i64))
}
