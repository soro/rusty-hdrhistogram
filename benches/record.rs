extern crate hdrhistogram;
extern crate rand;

#[path = "common/mod.rs"]
mod common;

use criterion::{criterion_group, criterion_main, Criterion};
use hdrhistogram::concurrent::recorder;
use hdrhistogram::concurrent::{
    ConcurrentDoubleHistogram, FixedConcurrentHistogram, ResizableConcurrentHistogram, SaturatingConcurrentDoubleHistogram,
};
use hdrhistogram::st::{DoubleHistogram, Histogram, HistogramWithCounter, SaturatingDoubleHistogram};
use rand::Rng;
use std::hint::black_box;

const MOSTLY_CLAMPED_VALUE_COUNT: usize = 1024;

fn mostly_clamped_double_values() -> Vec<f64> {
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    let mut values = Vec::with_capacity(MOSTLY_CLAMPED_VALUE_COUNT);
    for i in 0..MOSTLY_CLAMPED_VALUE_COUNT {
        state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        if state & 0b11 == 0 {
            values.push((common::TEST_VALUE_LEVEL + ((i as u64) & 0x800)) as f64);
        } else {
            values.push(f64::MAX);
        }
    }
    values
}

#[inline(always)]
fn next_mostly_clamped_value(values: &[f64], i: &mut usize) -> f64 {
    let value = values[*i & (MOSTLY_CLAMPED_VALUE_COUNT - 1)];
    *i = i.wrapping_add(1);
    value
}

fn record_value_histogram_u64(c: &mut Criterion) {
    c.bench_function("record_value_histogram_u64", |b| {
        let mut histogram = Histogram::with_low_high_sigvdig(1, common::HIGHEST_TRACKABLE_VALUE, common::SIGNIFICANT_VALUE_DIGITS).unwrap();
        let mut i = 0_u64;

        b.iter(|| {
            let value = black_box(common::next_recording_value(&mut i));
            histogram.record_value(value).unwrap();
        })
    });
}

fn record_value_histogram_u32(c: &mut Criterion) {
    c.bench_function("record_value_histogram_u32", |b| {
        let mut histogram =
            HistogramWithCounter::<u32>::with_low_high_sigvdig(1, common::HIGHEST_TRACKABLE_VALUE, common::SIGNIFICANT_VALUE_DIGITS)
                .unwrap();
        let mut i = 0_u64;

        b.iter(|| {
            let value = black_box(common::next_recording_value(&mut i));
            histogram.record_value(value).unwrap();
        })
    });
}

fn record_value_fixed_concurrent_histogram(c: &mut Criterion) {
    c.bench_function("record_value_fixed_concurrent_histogram", |b| {
        let histogram =
            FixedConcurrentHistogram::with_low_high_sigvdig(1, common::HIGHEST_TRACKABLE_VALUE, common::SIGNIFICANT_VALUE_DIGITS).unwrap();
        let mut i = 0_u64;

        b.iter(|| {
            let value = black_box(common::next_recording_value(&mut i));
            histogram.record_value(value).unwrap();
        })
    });
}

fn record_value_resizable_concurrent_histogram(c: &mut Criterion) {
    c.bench_function("record_value_resizable_concurrent_histogram", |b| {
        let histogram =
            ResizableConcurrentHistogram::with_low_high_sigvdig(1, common::HIGHEST_TRACKABLE_VALUE, common::SIGNIFICANT_VALUE_DIGITS)
                .unwrap();
        let mut i = 0_u64;

        b.iter(|| {
            let value = black_box(common::next_recording_value(&mut i));
            histogram.record_value(value).unwrap();
        })
    });
}

fn record_value_fixed_recorder(c: &mut Criterion) {
    c.bench_function("record_value_fixed_recorder", |b| {
        let recorder = recorder::fixed_with_low_high_sigvdig(1, common::HIGHEST_TRACKABLE_VALUE, common::SIGNIFICANT_VALUE_DIGITS).unwrap();
        let mut i = 0_u64;

        b.iter(|| {
            let value = black_box(common::next_recording_value(&mut i));
            recorder.record_value(value).unwrap();
        })
    });
}

fn record_value_resizable_recorder(c: &mut Criterion) {
    c.bench_function("record_value_resizable_recorder", |b| {
        let recorder =
            recorder::resizable_with_low_high_sigvdig(1, common::HIGHEST_TRACKABLE_VALUE, common::SIGNIFICANT_VALUE_DIGITS).unwrap();
        let mut i = 0_u64;

        b.iter(|| {
            let value = black_box(common::next_recording_value(&mut i));
            recorder.record_value(value).unwrap();
        })
    });
}

fn record_value_single_writer_recorder(c: &mut Criterion) {
    c.bench_function("record_value_single_writer_recorder", |b| {
        let recorder =
            recorder::single_writer_with_low_high_sigvdig(1, common::HIGHEST_TRACKABLE_VALUE, common::SIGNIFICANT_VALUE_DIGITS).unwrap();
        let mut i = 0_u64;

        b.iter(|| {
            let value = black_box(common::next_recording_value(&mut i));
            recorder.record_value(value).unwrap();
        })
    });
}

fn record_value_double_histogram(c: &mut Criterion) {
    c.bench_function("record_value_double_histogram", |b| {
        let mut histogram =
            DoubleHistogram::with_highest_to_lowest_value_ratio(common::HIGHEST_TRACKABLE_VALUE, common::SIGNIFICANT_VALUE_DIGITS).unwrap();
        let mut i = 0_u64;

        b.iter(|| {
            let value = black_box(common::next_recording_value(&mut i) as f64);
            histogram.record_value(value).unwrap();
        })
    });
}

fn record_value_saturating_double_histogram(c: &mut Criterion) {
    c.bench_function("record_value_saturating_double_histogram", |b| {
        let mut histogram = SaturatingDoubleHistogram::with_highest_to_lowest_value_ratio(
            common::HIGHEST_TRACKABLE_VALUE,
            common::SIGNIFICANT_VALUE_DIGITS,
        )
        .unwrap();
        let mut i = 0_u64;

        b.iter(|| {
            let value = black_box(common::next_recording_value(&mut i) as f64);
            histogram.record_value(value).unwrap();
        })
    });
}

fn record_out_of_range_saturating_double_histogram(c: &mut Criterion) {
    c.bench_function("record_out_of_range_saturating_double_histogram", |b| {
        let mut histogram = SaturatingDoubleHistogram::with_highest_to_lowest_value_ratio(
            common::HIGHEST_TRACKABLE_VALUE,
            common::SIGNIFICANT_VALUE_DIGITS,
        )
        .unwrap();

        b.iter(|| {
            histogram.record_value(black_box(f64::MAX)).unwrap();
        })
    });
}

fn record_mostly_clamped_saturating_double_histogram(c: &mut Criterion) {
    c.bench_function("record_mostly_clamped_saturating_double_histogram", |b| {
        let mut histogram = SaturatingDoubleHistogram::with_highest_to_lowest_value_ratio(
            common::HIGHEST_TRACKABLE_VALUE,
            common::SIGNIFICANT_VALUE_DIGITS,
        )
        .unwrap();
        let values = mostly_clamped_double_values();
        let mut i = 0_usize;

        b.iter(|| {
            let value = black_box(next_mostly_clamped_value(&values, &mut i));
            histogram.record_value(value).unwrap();
        })
    });
}

fn record_value_concurrent_double_histogram(c: &mut Criterion) {
    c.bench_function("record_value_concurrent_double_histogram", |b| {
        let histogram = ConcurrentDoubleHistogram::with_highest_to_lowest_value_ratio(
            common::HIGHEST_TRACKABLE_VALUE,
            common::SIGNIFICANT_VALUE_DIGITS,
        )
        .unwrap();
        let mut i = 0_u64;

        b.iter(|| {
            let value = black_box(common::next_recording_value(&mut i) as f64);
            histogram.record_value(value).unwrap();
        })
    });
}

fn record_value_saturating_concurrent_double_histogram(c: &mut Criterion) {
    c.bench_function("record_value_saturating_concurrent_double_histogram", |b| {
        let histogram = SaturatingConcurrentDoubleHistogram::with_highest_to_lowest_value_ratio(
            common::HIGHEST_TRACKABLE_VALUE,
            common::SIGNIFICANT_VALUE_DIGITS,
        )
        .unwrap();
        let mut i = 0_u64;

        b.iter(|| {
            let value = black_box(common::next_recording_value(&mut i) as f64);
            histogram.record_value(value).unwrap();
        })
    });
}

fn record_out_of_range_saturating_concurrent_double_histogram(c: &mut Criterion) {
    c.bench_function("record_out_of_range_saturating_concurrent_double_histogram", |b| {
        let histogram = SaturatingConcurrentDoubleHistogram::with_highest_to_lowest_value_ratio(
            common::HIGHEST_TRACKABLE_VALUE,
            common::SIGNIFICANT_VALUE_DIGITS,
        )
        .unwrap();

        b.iter(|| {
            histogram.record_value(black_box(f64::MAX)).unwrap();
        })
    });
}

fn record_mostly_clamped_saturating_concurrent_double_histogram(c: &mut Criterion) {
    c.bench_function("record_mostly_clamped_saturating_concurrent_double_histogram", |b| {
        let histogram = SaturatingConcurrentDoubleHistogram::with_highest_to_lowest_value_ratio(
            common::HIGHEST_TRACKABLE_VALUE,
            common::SIGNIFICANT_VALUE_DIGITS,
        )
        .unwrap();
        let values = mostly_clamped_double_values();
        let mut i = 0_usize;

        b.iter(|| {
            let value = black_box(next_mostly_clamped_value(&values, &mut i));
            histogram.record_value(value).unwrap();
        })
    });
}

fn record_value_double_recorder(c: &mut Criterion) {
    c.bench_function("record_value_double_recorder", |b| {
        let recorder =
            recorder::double_with_highest_to_lowest_value_ratio(common::HIGHEST_TRACKABLE_VALUE, common::SIGNIFICANT_VALUE_DIGITS).unwrap();
        let mut i = 0_u64;

        b.iter(|| {
            let value = black_box(common::next_recording_value(&mut i) as f64);
            recorder.record_value(value).unwrap();
        })
    });
}

fn record_value_saturating_double_recorder(c: &mut Criterion) {
    c.bench_function("record_value_saturating_double_recorder", |b| {
        let recorder = recorder::saturating_double_with_highest_to_lowest_value_ratio(
            common::HIGHEST_TRACKABLE_VALUE,
            common::SIGNIFICANT_VALUE_DIGITS,
        )
        .unwrap();
        let mut i = 0_u64;

        b.iter(|| {
            let value = black_box(common::next_recording_value(&mut i) as f64);
            recorder.record_value(value).unwrap();
        })
    });
}

fn record_out_of_range_saturating_double_recorder(c: &mut Criterion) {
    c.bench_function("record_out_of_range_saturating_double_recorder", |b| {
        let recorder = recorder::saturating_double_with_highest_to_lowest_value_ratio(
            common::HIGHEST_TRACKABLE_VALUE,
            common::SIGNIFICANT_VALUE_DIGITS,
        )
        .unwrap();

        b.iter(|| {
            recorder.record_value(black_box(f64::MAX)).unwrap();
        })
    });
}

fn record_mostly_clamped_saturating_double_recorder(c: &mut Criterion) {
    c.bench_function("record_mostly_clamped_saturating_double_recorder", |b| {
        let recorder = recorder::saturating_double_with_highest_to_lowest_value_ratio(
            common::HIGHEST_TRACKABLE_VALUE,
            common::SIGNIFICANT_VALUE_DIGITS,
        )
        .unwrap();
        let values = mostly_clamped_double_values();
        let mut i = 0_usize;

        b.iter(|| {
            let value = black_box(next_mostly_clamped_value(&values, &mut i));
            recorder.record_value(value).unwrap();
        })
    });
}

fn record_value_single_writer_double_recorder(c: &mut Criterion) {
    c.bench_function("record_value_single_writer_double_recorder", |b| {
        let recorder = recorder::single_writer_double_with_highest_to_lowest_value_ratio(
            common::HIGHEST_TRACKABLE_VALUE,
            common::SIGNIFICANT_VALUE_DIGITS,
        )
        .unwrap();
        let mut i = 0_u64;

        b.iter(|| {
            let value = black_box(common::next_recording_value(&mut i) as f64);
            recorder.record_value(value).unwrap();
        })
    });
}

fn record_value_saturating_single_writer_double_recorder(c: &mut Criterion) {
    c.bench_function("record_value_saturating_single_writer_double_recorder", |b| {
        let recorder = recorder::saturating_single_writer_double_with_highest_to_lowest_value_ratio(
            common::HIGHEST_TRACKABLE_VALUE,
            common::SIGNIFICANT_VALUE_DIGITS,
        )
        .unwrap();
        let mut i = 0_u64;

        b.iter(|| {
            let value = black_box(common::next_recording_value(&mut i) as f64);
            recorder.record_value(value).unwrap();
        })
    });
}

fn record_out_of_range_saturating_single_writer_double_recorder(c: &mut Criterion) {
    c.bench_function("record_out_of_range_saturating_single_writer_double_recorder", |b| {
        let recorder = recorder::saturating_single_writer_double_with_highest_to_lowest_value_ratio(
            common::HIGHEST_TRACKABLE_VALUE,
            common::SIGNIFICANT_VALUE_DIGITS,
        )
        .unwrap();

        b.iter(|| {
            recorder.record_value(black_box(f64::MAX)).unwrap();
        })
    });
}

fn record_mostly_clamped_saturating_single_writer_double_recorder(c: &mut Criterion) {
    c.bench_function("record_mostly_clamped_saturating_single_writer_double_recorder", |b| {
        let recorder = recorder::saturating_single_writer_double_with_highest_to_lowest_value_ratio(
            common::HIGHEST_TRACKABLE_VALUE,
            common::SIGNIFICANT_VALUE_DIGITS,
        )
        .unwrap();
        let values = mostly_clamped_double_values();
        let mut i = 0_usize;

        b.iter(|| {
            let value = black_box(next_mostly_clamped_value(&values, &mut i));
            recorder.record_value(value).unwrap();
        })
    });
}

fn record_precalc_random_values_with_1_count_u64(c: &mut Criterion) {
    c.bench_function("record_precalc_random_values_with_1_count_u64", |b| {
        let mut histogram = Histogram::with_low_high_sigvdig(1, u64::MAX, 3).unwrap();
        let mut values = Vec::<u64>::new();
        let mut rng = rand::thread_rng();

        for _ in 0..3000000 {
            values.push(rng.gen::<u64>());
        }

        b.iter(|| {
            for value in values.iter() {
                histogram.record_value(black_box(*value)).unwrap()
            }
        })
    });
}

fn bench_percentile(c: &mut Criterion) {
    c.bench_function("bench_percentile", |b| {
        let mut histogram = Histogram::with_low_high_sigvdig(1, u64::MAX, 3).unwrap();
        let mut indices = Vec::<u64>::new();
        let mut rng = rand::thread_rng();

        for _ in 0..1000000 {
            indices.push(rng.gen());
        }

        for value in indices.iter() {
            histogram.record_value(*value).unwrap()
        }

        b.iter(|| {
            for tenth_percentile in 0..=1000 {
                black_box(histogram.get_value_at_percentile(black_box(tenth_percentile as f64 / 10.0)));
            }
        })
    });
}

fn percentile_iter(c: &mut Criterion) {
    c.bench_function("percentile_iter", |b| {
        let mut histogram = Histogram::with_low_high_sigvdig(1, u64::MAX, 3).unwrap();
        let length = 1000000;

        for value in 1..=length {
            histogram.record_value(value).unwrap();
        }

        let percentile_ticks_per_half_distance = 1000;
        b.iter(|| {
            for _ in 1..30 {
                black_box(histogram.percentiles(percentile_ticks_per_half_distance).last());
            }
        })
    });
}

criterion_group!(
    benches,
    record_value_histogram_u64,
    record_value_histogram_u32,
    record_value_fixed_concurrent_histogram,
    record_value_resizable_concurrent_histogram,
    record_value_fixed_recorder,
    record_value_resizable_recorder,
    record_value_single_writer_recorder,
    record_value_double_histogram,
    record_value_saturating_double_histogram,
    record_out_of_range_saturating_double_histogram,
    record_mostly_clamped_saturating_double_histogram,
    record_value_concurrent_double_histogram,
    record_value_saturating_concurrent_double_histogram,
    record_out_of_range_saturating_concurrent_double_histogram,
    record_mostly_clamped_saturating_concurrent_double_histogram,
    record_value_double_recorder,
    record_value_saturating_double_recorder,
    record_out_of_range_saturating_double_recorder,
    record_mostly_clamped_saturating_double_recorder,
    record_value_single_writer_double_recorder,
    record_value_saturating_single_writer_double_recorder,
    record_out_of_range_saturating_single_writer_double_recorder,
    record_mostly_clamped_saturating_single_writer_double_recorder,
    record_precalc_random_values_with_1_count_u64,
    bench_percentile,
    percentile_iter,
);
criterion_main!(benches);
