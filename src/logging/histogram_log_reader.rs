use serialization::DeserializableHistogram;
use lazycell::LazyCell;
use std::io::Cursor;
use core::*;
use std::time::SystemTime;
use std::marker::PhantomData;
use base64;

//pub enum LogLine<'a, C, H> {
//    Comment()
//}

struct LogEntry<'a, C: 'a, H> {
    pub start_time: SystemTime,
    pub end_time: SystemTime,
    pub max_value: &'a u64,
    raw_histogram: &'a [u8],
    histogram: LazyCell<Result<H, LogReadError>>,
    phantom: PhantomData<&'a C>
}

impl<'a, C: Counter, H: DeserializableHistogram<C>> LogEntry<'a, C, H> {
    fn deserialize_histogram(raw_histogram: &[u8]) -> Result<H, LogReadError> {
        let decoded = base64::decode(raw_histogram)?;
        Ok(H::deserialize_from(&mut Cursor::new(decoded), C::zero())?)
    }

    pub fn histogram(&mut self) -> &Result<H, LogReadError> {
        let histogram = &self.histogram;
        let raw_histogram = &mut self.raw_histogram;
        let fulfill = || { Self::deserialize_histogram(raw_histogram) };
        histogram.borrow_with(fulfill)
    }
}

struct LogIteratorState {
    last_segment_start_time: SystemTime,
    base_time: SystemTime,
    max_value_unit_ratio: f64,
}

