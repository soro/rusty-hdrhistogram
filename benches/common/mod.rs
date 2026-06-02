#![allow(dead_code)]

use hdrhistogram::st::{DoubleHistogram, Histogram};

pub const HIGHEST_TRACKABLE_VALUE: u64 = 3_600 * 1_000 * 1_000;
pub const SIGNIFICANT_VALUE_DIGITS: u8 = 3;
pub const TEST_VALUE_LEVEL: u64 = 12_340;

#[derive(Clone, Copy)]
pub enum LatencySeries {
    Mixed,
    Dense10k,
    Dense100k,
    Sparse5,
    Sparse8,
    Quadratic,
    Cubic,
    MixedPlusSparse8,
}

#[inline(always)]
pub fn next_recording_value(i: &mut u64) -> u64 {
    let value = TEST_VALUE_LEVEL + (*i & 0x800);
    *i = i.wrapping_add(1);
    value
}

pub fn latency_series(series: LatencySeries) -> Vec<u64> {
    match series {
        LatencySeries::Mixed => mixed_latencies(),
        LatencySeries::Dense10k => dense_latencies(10_000),
        LatencySeries::Dense100k => dense_latencies(100_000),
        LatencySeries::Sparse5 => sparse_latencies(5),
        LatencySeries::Sparse8 => sparse_latencies(8),
        LatencySeries::Quadratic => power_latencies(2),
        LatencySeries::Cubic => power_latencies(3),
        LatencySeries::MixedPlusSparse8 => {
            let mut values = mixed_latencies();
            values.extend(sparse_latencies(8));
            values
        }
    }
}

pub fn histogram_u64(series: LatencySeries, significant_value_digits: u8) -> Histogram<u64> {
    let mut histogram = Histogram::<u64>::new(significant_value_digits).unwrap();
    histogram.set_auto_resize(true);
    for value in latency_series(series) {
        histogram.record_value(value).unwrap();
    }
    histogram
}

pub fn histogram_u32(series: LatencySeries, significant_value_digits: u8) -> Histogram<u32> {
    let mut histogram = Histogram::<u32>::new(significant_value_digits).unwrap();
    histogram.set_auto_resize(true);
    for value in latency_series(series) {
        histogram.record_value(value).unwrap();
    }
    histogram
}

pub fn double_histogram(series: LatencySeries, significant_value_digits: u8) -> DoubleHistogram {
    let mut histogram = DoubleHistogram::new(significant_value_digits).unwrap();
    for value in latency_series(series) {
        histogram.record_value(value as f64).unwrap();
    }
    histogram
}

fn dense_latencies(length: u64) -> Vec<u64> {
    (1..=length).map(|i| 1_000 * i).collect()
}

fn sparse_latencies(length: u64) -> Vec<u64> {
    (1..=length)
        .map(|i| {
            let mut value = 1_u64;
            for _ in 0..i {
                value *= i;
            }
            value
        })
        .collect()
}

fn power_latencies(power: u32) -> Vec<u64> {
    (1..=10_000)
        .map(|i| i64::pow(i, power) as u64)
        .filter(|value| *value < i32::MAX as u64)
        .collect()
}

fn mixed_latencies() -> Vec<u64> {
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    (0..1_500)
        .map(|i| {
            state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            let base = 80 + ((state >> 33) % 1_700);
            if i % 173 == 0 {
                base + 12_000 + (state % 8_000)
            } else if i % 41 == 0 {
                base + 2_000 + (state % 3_000)
            } else {
                base
            }
        })
        .collect()
}

#[cfg(feature = "encoding-base64")]
pub fn histogram_log(intervals: usize, significant_value_digits: u8) -> String {
    use hdrhistogram::encoding::HistogramLogWriter;

    let mut output = Vec::new();
    {
        let mut writer = HistogramLogWriter::new(&mut output);
        writer.write_format_version().unwrap();
        writer.write_start_time(1_700_000_000.0).unwrap();
        writer.write_base_time(1_700_000_000.0).unwrap();
        writer.write_legend().unwrap();
        for interval in 0..intervals {
            let mut histogram = Histogram::<u64>::new(significant_value_digits).unwrap();
            histogram.set_auto_resize(true);
            for value in latency_series(LatencySeries::Mixed) {
                histogram.record_value(value + interval as u64).unwrap();
            }
            let start = 1_700_000_000.0 + interval as f64;
            writer.write_interval(&histogram, start, start + 1.0).unwrap();
        }
    }
    String::from_utf8(output).unwrap()
}
