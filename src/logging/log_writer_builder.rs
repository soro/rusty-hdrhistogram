use std::time::{UNIX_EPOCH, SystemTime};
use std::io::Write;
use std::marker::PhantomData;
use logging::histogram_log_writer::HistogramLogWriter;
use core::*;
use logging::util::*;

const DEFAULT_MAX_VALUE_UNIT_RATIO: f64 = 1000000.0;
const HISTOGRAM_LOG_FORMAT_VERSION: &'static str = "1.3";

pub enum Buildable {}
pub enum NeedsSink {}

struct BuilderState<'a, T> {
    base_time: Option<SystemTime>,
    max_value_unit_ratio: Option<f64>,
    sink: Option<T>,
    write_format_version: bool,
    write_legend: bool,
    start_time: Option<SystemTime>,
    header_comment: Option<&'a str>,
}

impl<'a, T> BuilderState<'a, T> {
    pub fn new() -> BuilderState<'a, T> { BuilderState {
        base_time: None,
        max_value_unit_ratio: None,
        sink: None,
        write_format_version: false,
        write_legend: false,
        start_time: None,
        header_comment: None,
    }}
}

pub struct LogWriterBuilder<'a, T, B> {
    state: BuilderState<'a, T>,
    phantom: PhantomData<B>,
}

pub fn builder<'a, T>() -> LogWriterBuilder<'a, T, NeedsSink> {
    LogWriterBuilder {
        state: BuilderState::new(),
        phantom: PhantomData,
    }
}

impl<'a, T: Write, B> LogWriterBuilder<'a, T, B> {
    pub fn write_format_version(mut self) -> LogWriterBuilder<'a, T, B> {
        self.state.write_format_version = true;
        self
    }

    pub fn write_legend(mut self) -> LogWriterBuilder<'a, T, B> {
        self.state.write_legend = true;
        self
    }

    pub fn write_start_time(mut self, time: SystemTime) -> LogWriterBuilder<'a, T, B> {
        self.state.start_time = Some(time);
        self
    }

    pub fn header_comment<'b: 'a>(mut self, comment: &'b str) -> LogWriterBuilder<'a, T, B> {
        self.state.header_comment = Some(comment);
        self
    }

    pub fn base_time(mut self, base_time: SystemTime) -> LogWriterBuilder<'a, T, B> {
        self.state.base_time = Some(base_time);
        self
    }

    pub fn max_value_unit_ratio(mut self, max_value_unit_ratio: f64) -> LogWriterBuilder<'a, T, B> {
        self.state.max_value_unit_ratio = Some(max_value_unit_ratio);
        self
    }

    pub fn sink(mut self, sink: T) -> LogWriterBuilder<'a, T, Buildable> {
        self.state.sink = Some(sink);
        LogWriterBuilder { state: self.state, phantom: PhantomData }
    }
}

impl<'a, T: Write> LogWriterBuilder<'a, T, Buildable> {
    pub fn build(self) -> Result<HistogramLogWriter<T>, LoggingError> {
        let slf = self.write_header_fields()?;
        Ok(slf.build_no_header())
    }

    pub fn build_no_header(self) -> HistogramLogWriter<T> {
        let state = self.state;
        HistogramLogWriter::new(
            state.sink.unwrap(),
            state.base_time.unwrap_or(UNIX_EPOCH),
            state.max_value_unit_ratio.unwrap_or(DEFAULT_MAX_VALUE_UNIT_RATIO)
        )
    }

    pub fn build_with_header(mut self) -> Result<HistogramLogWriter<T>, LoggingError> {
        {
            let state = &mut self.state;
            if state.start_time.is_none() { state.start_time = Some(SystemTime::now()); }
            state.write_format_version = true;
            state.write_legend = true;
        }

        self.build()
    }

    fn write_header_fields(mut self) -> Result<Self, LoggingError> {
        match self.state.sink {
            Some(ref mut sink) => {
                if let Some(c) = self.state.header_comment {
                    sink.write_all(c.as_bytes())?;
                }
                if self.state.write_format_version {
                    sink.write_fmt(format_args!("[Histogram log format version {}]", HISTOGRAM_LOG_FORMAT_VERSION))?
                }
                if let Some(t) = self.state.start_time {
                    let start_duration = t.duration_since(UNIX_EPOCH).map(duration_as_float)?;
                    writeln!(sink, "#[StartTime: {:.3} (seconds since epoch)]", start_duration)?;
                };
                if self.state.write_legend {
                    writeln!(sink, r#""StartTimestamp","Interval_Length","Interval_Max","Interval_Compressed_Histogram""#)?;
                }
                if let Some(t) = self.state.base_time {
                    let duration = if t != UNIX_EPOCH {
                        duration_as_float(t.duration_since(UNIX_EPOCH).unwrap())
                    } else { 0.0 };
                    writeln!(sink, "#[BaseTime: {:.3} (seconds since epoch)]", duration)?;
                };
            }
            _ => panic!("impossible")
        }
        Ok(self)
    }
}
