use std::io::{self, Write};
use std::time::{SystemTime, UNIX_EPOCH};
use std::fmt::Arguments;
use serialization::SerializableHistogram;
use core::*;
use logging::util::*;
use base64;


pub struct HistogramLogWriter<T> {
    target_buffer: Vec<u8>,
    encode_buffer: Vec<u8>, // TODO: get rid of this
    sink: T,
    base_time: SystemTime,
    max_value_unit_ratio: f64,
}



// this is thread safe according to docs, regex match state is synchronized
fn has_delimiters(text: &str) -> bool {
    text.chars().any(|c| { c == ',' || c == ' ' || c == '\r' || c == '\n' })
}


impl<T: Write> HistogramLogWriter<T> {
    pub fn new(sink: T, base_time: SystemTime, max_value_unit_ratio: f64) -> HistogramLogWriter<T> {
        HistogramLogWriter {
            target_buffer: Vec::with_capacity(64),
            encode_buffer: Vec::with_capacity(64),
            sink,
            base_time,
            max_value_unit_ratio,
        }

    }

    pub fn write_fmt(&mut self, args: Arguments) -> Result<(), LoggingError> {
        Ok(self.sink.write_fmt(args)?)
    }

    pub fn set_base_time(&mut self, time: SystemTime) {
        self.base_time = time
        // TODO: should also write to log
    }

    pub fn get_base_time(&self) -> SystemTime { self.base_time }

    pub fn flush(&mut self) -> Result<(), io::Error> {
        self.sink.flush()
    }

    pub fn output_comment(&mut self, comment: &str) -> Result<(), LoggingError> {
        self.sink.write_all(comment.as_bytes())?;
        Ok(write!(self.sink, "\n")?)
    }

    pub fn log<C, H>(&mut self, histogram: &mut H) -> Result<(), LoggingError> where C: Counter, H: SerializableHistogram<C> {
        self.output_interval_histogram(histogram)
    }

    pub fn log_with_start_end<C, H>(&mut self, histogram: &mut H, start: SystemTime, end: SystemTime) -> Result<(), LoggingError> where C: Counter, H: SerializableHistogram<C> {
        self.write_log_entry(histogram, Some(start), Some(end))
    }

    pub fn output_interval_histogram<C, H>(&mut self, histogram: &mut H) -> Result<(), LoggingError> where C: Counter, H: SerializableHistogram<C> {
        let (start, end) = {
            let meta_data = histogram.meta_data();
            (meta_data.start_timestamp, meta_data.end_timestamp)
        };
        self.write_log_entry(histogram, start, end)
    }

    fn write_log_entry<C, H>(
        &mut self,
        histogram: &mut H,
        start_time: Option<SystemTime>,
        end_time: Option<SystemTime>
    ) -> Result<(), LoggingError> where C: Counter, H: SerializableHistogram<C> {
        let existing_capacity = self.target_buffer.capacity();
        let required_capacity = histogram.required_buffer_capacity();
        if existing_capacity < required_capacity {
            self.target_buffer.reserve(required_capacity - existing_capacity);
        }
        self.target_buffer.clear();

        let bytes_written = histogram.serialize_into_compressed(&mut self.target_buffer)?;
        let slice = &self.target_buffer.as_slice()[0..bytes_written];

        let start_duration = match start_time {
            Some(s) => {
                let duration = s.duration_since(self.base_time)?;
                duration_as_float(duration)
            }
            _ => 0.0
        };
        let start = start_time.unwrap_or(UNIX_EPOCH);
        let end_duration = end_time.unwrap_or(SystemTime::now()).duration_since(start)
            .map(duration_as_float)
            .unwrap_or(start_duration);

        let double_max_value = histogram.get_max_value() as f64;
        let scaled_max_value = double_max_value / self.max_value_unit_ratio;

        match histogram.meta_data().tag {
            Some(ref t) if !has_delimiters(&t) => write!(self.sink, "Tag={},", t)?,
            _ => {}
        }

        write!(self.sink, "{:.3},{:.3},{:.3},", start_duration, end_duration, scaled_max_value)?;

        let current_encode_capacity = self.encode_buffer.capacity();
        let required_encode_capacity =  self.target_buffer.len() * 4 / 3 + 4;
        if current_encode_capacity < required_encode_capacity {
            self.encode_buffer.reserve(required_encode_capacity - current_encode_capacity);
        }
        self.encode_buffer.clear();

        base64::encode_config_slice(slice, base64::STANDARD, &mut self.encode_buffer);

        self.sink.write_all(self.encode_buffer.as_slice())?;
        write!(self.sink, "\n")?;

        Ok(())
    }


}




//    pub fn log_with_start_end<T: SerializableHistogram>(&mut self, histogram: T, start: SystemTime, end: SystemTime) -> Result<(), LoggingError> {
//
//    }


//
//        String tag = histogram.getTag();
//        if (tag == null) {
//            log.format(Locale.US, "%.3f,%.3f,%.3f,%s\n",
//                       startTimeStampSec,
//                       endTimeStampSec - startTimeStampSec,
//                       histogram.getMaxValueAsDouble() / maxValueUnitRatio,
//                       Base64Helper.printBase64Binary(compressedArray)
//            );
//        } else {
//            containsDelimeterMatcher.reset(tag);
//            if (containsDelimeterMatcher.matches()) {
//                throw new IllegalArgumentException("Tag string cannot contain commas, spaces, or line breaks");
//            }
//            log.format(Locale.US, "Tag=%s,%.3f,%.3f,%.3f,%s\n",
//                       tag,
//                       startTimeStampSec,
//                       endTimeStampSec - startTimeStampSec,
//                       histogram.getMaxValueAsDouble() / maxValueUnitRatio,
//                       Base64Helper.printBase64Binary(compressedArray)
//            );
//        }
