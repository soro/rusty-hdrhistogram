#![feature(test)]
extern crate hdrhistogram;
extern crate rand;
extern crate test;

use self::test::Bencher;
use hdrhistogram::st::Histogram;
use rand::Rng;

#[bench]
fn record_precalc_random_values_with_1_count_u64(b: &mut Bencher) {
    let mut h = Histogram::<u64>::with_low_high_sigvdig(1, u64::MAX, 3).unwrap();
    let mut values = Vec::<u64>::new();
    let mut rng = rand::thread_rng();

    for _ in 0..3000000 {
        values.push(rng.gen::<u64>());
    }

    b.iter(|| {
        for i in values.iter() {
            h.record_value(*i).unwrap()
        }
    })
}

#[bench]
fn bench_percentile(b: &mut Bencher) {
    let mut h = Histogram::<u64>::with_low_high_sigvdig(1, u64::MAX, 3).unwrap();
    let mut indices = Vec::<u64>::new();
    let mut rng = rand::thread_rng();

    for _ in 0..1000000 {
        indices.push(rng.gen());
    }

    for i in indices.iter() {
        h.record_value(*i).unwrap()
    }

    b.iter(|| {
        for i in 1..1000000 {
            h.get_value_at_percentile(i as f64);
        }
    })
}

#[bench]
fn percentile_iter(b: &mut Bencher) {
    let mut histogram = Histogram::<u64>::with_low_high_sigvdig(1, u64::MAX, 3).unwrap();
    let length = 1000000;

    for value in 1..=length {
        histogram.record_value(value).unwrap();
    }

    let percentile_ticks_per_half_distance = 1000;
    b.iter(|| {
        for _ in 1..30 {
            histogram
                .percentiles(percentile_ticks_per_half_distance)
                .last();
        }
    })
}
