extern crate hdrhistogram;

#[path = "common/mod.rs"]
mod common;

use common::LatencySeries;
use criterion::{criterion_group, criterion_main, Criterion};
use hdrhistogram::encoding;
use std::hint::black_box;

macro_rules! bench_encode_v2_u64 {
    ($criterion:expr, $name:ident, $series:ident, $digits:expr) => {{
        let histogram = common::histogram_u64(LatencySeries::$series, $digits);

        $criterion.bench_function(stringify!($name), |b| {
            b.iter(|| {
                let encoded = encoding::encode_histogram_v2(black_box(&histogram)).unwrap();
                black_box(encoded);
            })
        });
    }};
}

macro_rules! bench_encode_v2_u32 {
    ($criterion:expr, $name:ident, $series:ident, $digits:expr) => {{
        let histogram = common::histogram_u32(LatencySeries::$series, $digits);

        $criterion.bench_function(stringify!($name), |b| {
            b.iter(|| {
                let encoded = encoding::encode_histogram_v2(black_box(&histogram)).unwrap();
                black_box(encoded);
            })
        });
    }};
}

macro_rules! bench_roundtrip_v2_u64 {
    ($criterion:expr, $name:ident, $series:ident, $digits:expr) => {{
        let histogram = common::histogram_u64(LatencySeries::$series, $digits);

        $criterion.bench_function(stringify!($name), |b| {
            b.iter(|| {
                let encoded = encoding::encode_histogram_v2(black_box(&histogram)).unwrap();
                let decoded = encoding::decode_histogram_v2(black_box(&encoded)).unwrap();
                black_box(decoded.get_total_count());
            })
        });
    }};
}

macro_rules! bench_encode_v2_double {
    ($criterion:expr, $name:ident, $series:ident, $digits:expr) => {{
        let histogram = common::double_histogram(LatencySeries::$series, $digits);

        $criterion.bench_function(stringify!($name), |b| {
            b.iter(|| {
                let encoded = encoding::encode_double_histogram_v2(black_box(&histogram)).unwrap();
                black_box(encoded);
            })
        });
    }};
}

macro_rules! bench_roundtrip_v2_double {
    ($criterion:expr, $name:ident, $series:ident, $digits:expr) => {{
        let histogram = common::double_histogram(LatencySeries::$series, $digits);

        $criterion.bench_function(stringify!($name), |b| {
            b.iter(|| {
                let encoded = encoding::encode_double_histogram_v2(black_box(&histogram)).unwrap();
                let decoded = encoding::decode_double_histogram_v2(black_box(&encoded)).unwrap();
                black_box(decoded.get_total_count());
            })
        });
    }};
}

#[cfg(feature = "encoding-compression")]
macro_rules! bench_encode_compressed_u64 {
    ($criterion:expr, $name:ident, $series:ident, $digits:expr) => {{
        let histogram = common::histogram_u64(LatencySeries::$series, $digits);

        $criterion.bench_function(stringify!($name), |b| {
            b.iter(|| {
                let encoded = encoding::encode_histogram_compressed(black_box(&histogram)).unwrap();
                black_box(encoded);
            })
        });
    }};
}

#[cfg(feature = "encoding-compression")]
macro_rules! bench_roundtrip_compressed_u64 {
    ($criterion:expr, $name:ident, $series:ident, $digits:expr) => {{
        let histogram = common::histogram_u64(LatencySeries::$series, $digits);

        $criterion.bench_function(stringify!($name), |b| {
            b.iter(|| {
                let encoded = encoding::encode_histogram_compressed(black_box(&histogram)).unwrap();
                let decoded = encoding::decode_histogram_compressed(black_box(&encoded)).unwrap();
                black_box(decoded.get_total_count());
            })
        });
    }};
}

#[cfg(feature = "encoding-compression")]
macro_rules! bench_encode_compressed_double {
    ($criterion:expr, $name:ident, $series:ident, $digits:expr) => {{
        let histogram = common::double_histogram(LatencySeries::$series, $digits);

        $criterion.bench_function(stringify!($name), |b| {
            b.iter(|| {
                let encoded = encoding::encode_double_histogram_compressed(black_box(&histogram)).unwrap();
                black_box(encoded);
            })
        });
    }};
}

#[cfg(feature = "encoding-compression")]
macro_rules! bench_roundtrip_compressed_double {
    ($criterion:expr, $name:ident, $series:ident, $digits:expr) => {{
        let histogram = common::double_histogram(LatencySeries::$series, $digits);

        $criterion.bench_function(stringify!($name), |b| {
            b.iter(|| {
                let encoded = encoding::encode_double_histogram_compressed(black_box(&histogram)).unwrap();
                let decoded = encoding::decode_double_histogram_compressed(black_box(&encoded)).unwrap();
                black_box(decoded.get_total_count());
            })
        });
    }};
}

#[cfg(feature = "encoding-base64")]
macro_rules! bench_encode_base64_u64 {
    ($criterion:expr, $name:ident, $series:ident, $digits:expr) => {{
        let histogram = common::histogram_u64(LatencySeries::$series, $digits);

        $criterion.bench_function(stringify!($name), |b| {
            b.iter(|| {
                let encoded = encoding::encode_histogram_base64(black_box(&histogram)).unwrap();
                black_box(encoded);
            })
        });
    }};
}

#[cfg(feature = "encoding-base64")]
macro_rules! bench_roundtrip_base64_u64 {
    ($criterion:expr, $name:ident, $series:ident, $digits:expr) => {{
        let histogram = common::histogram_u64(LatencySeries::$series, $digits);

        $criterion.bench_function(stringify!($name), |b| {
            b.iter(|| {
                let encoded = encoding::encode_histogram_base64(black_box(&histogram)).unwrap();
                let decoded = encoding::decode_histogram_base64(black_box(&encoded)).unwrap();
                black_box(decoded.get_total_count());
            })
        });
    }};
}

#[cfg(feature = "encoding-base64")]
macro_rules! bench_encode_base64_double {
    ($criterion:expr, $name:ident, $series:ident, $digits:expr) => {{
        let histogram = common::double_histogram(LatencySeries::$series, $digits);

        $criterion.bench_function(stringify!($name), |b| {
            b.iter(|| {
                let encoded = encoding::encode_double_histogram_base64(black_box(&histogram)).unwrap();
                black_box(encoded);
            })
        });
    }};
}

#[cfg(feature = "encoding-base64")]
macro_rules! bench_roundtrip_base64_double {
    ($criterion:expr, $name:ident, $series:ident, $digits:expr) => {{
        let histogram = common::double_histogram(LatencySeries::$series, $digits);

        $criterion.bench_function(stringify!($name), |b| {
            b.iter(|| {
                let encoded = encoding::encode_double_histogram_base64(black_box(&histogram)).unwrap();
                let decoded = encoding::decode_double_histogram_base64(black_box(&encoded)).unwrap();
                black_box(decoded.get_total_count());
            })
        });
    }};
}

fn encoding_benches(c: &mut Criterion) {
    bench_encode_v2_u64!(c, encode_v2_u64_mixed_digits_2, Mixed, 2);
    bench_encode_v2_u64!(c, encode_v2_u64_mixed_digits_3, Mixed, 3);
    bench_encode_v2_u64!(c, encode_v2_u64_dense_10k_digits_3, Dense10k, 3);
    bench_encode_v2_u64!(c, encode_v2_u64_dense_100k_digits_3, Dense100k, 3);
    bench_encode_v2_u64!(c, encode_v2_u64_sparse_8_digits_3, Sparse8, 3);
    bench_encode_v2_u64!(c, encode_v2_u64_quadratic_digits_3, Quadratic, 3);
    bench_encode_v2_u64!(c, encode_v2_u64_cubic_digits_3, Cubic, 3);
    bench_encode_v2_u64!(c, encode_v2_u64_mixed_plus_sparse_8_digits_3, MixedPlusSparse8, 3);

    bench_encode_v2_u32!(c, encode_v2_u32_mixed_digits_3, Mixed, 3);
    bench_encode_v2_u32!(c, encode_v2_u32_dense_100k_digits_3, Dense100k, 3);

    bench_roundtrip_v2_u64!(c, roundtrip_v2_u64_mixed_digits_3, Mixed, 3);
    bench_roundtrip_v2_u64!(c, roundtrip_v2_u64_dense_100k_digits_3, Dense100k, 3);

    bench_encode_v2_double!(c, encode_v2_double_mixed_digits_3, Mixed, 3);
    bench_encode_v2_double!(c, encode_v2_double_dense_100k_digits_3, Dense100k, 3);
    bench_roundtrip_v2_double!(c, roundtrip_v2_double_mixed_digits_3, Mixed, 3);

    #[cfg(feature = "encoding-compression")]
    {
        bench_encode_compressed_u64!(c, encode_compressed_u64_mixed_digits_2, Mixed, 2);
        bench_encode_compressed_u64!(c, encode_compressed_u64_mixed_digits_3, Mixed, 3);
        bench_encode_compressed_u64!(c, encode_compressed_u64_dense_100k_digits_3, Dense100k, 3);
        bench_encode_compressed_u64!(c, encode_compressed_u64_sparse_8_digits_3, Sparse8, 3);
        bench_roundtrip_compressed_u64!(c, roundtrip_compressed_u64_mixed_digits_3, Mixed, 3);
        bench_roundtrip_compressed_u64!(c, roundtrip_compressed_u64_dense_100k_digits_3, Dense100k, 3);
        bench_encode_compressed_double!(c, encode_compressed_double_mixed_digits_3, Mixed, 3);
        bench_roundtrip_compressed_double!(c, roundtrip_compressed_double_mixed_digits_3, Mixed, 3);
    }

    #[cfg(feature = "encoding-base64")]
    {
        bench_encode_base64_u64!(c, encode_base64_u64_mixed_digits_3, Mixed, 3);
        bench_encode_base64_u64!(c, encode_base64_u64_dense_100k_digits_3, Dense100k, 3);
        bench_roundtrip_base64_u64!(c, roundtrip_base64_u64_mixed_digits_3, Mixed, 3);
        bench_encode_base64_double!(c, encode_base64_double_mixed_digits_3, Mixed, 3);
        bench_roundtrip_base64_double!(c, roundtrip_base64_double_mixed_digits_3, Mixed, 3);
        generate_histogram_log_report_u64_mixed_100_intervals(c);
    }
}

#[cfg(feature = "encoding-base64")]
fn generate_histogram_log_report_u64_mixed_100_intervals(c: &mut Criterion) {
    let log = common::histogram_log(100, common::SIGNIFICANT_VALUE_DIGITS);
    let config = encoding::HistogramLogReportConfig {
        moving_window: Some(encoding::HistogramLogMovingWindowConfig {
            percentile_to_report: 99.0,
            length_sec: 10.0,
        }),
        ..encoding::HistogramLogReportConfig::default()
    };

    c.bench_function("generate_histogram_log_report_u64_mixed_100_intervals", |b| {
        b.iter(|| {
            let report = encoding::generate_histogram_log_report(black_box(log.lines()), black_box(&config)).unwrap();
            black_box(report.processed_interval_count);
        })
    });
}

criterion_group!(benches, encoding_benches);
criterion_main!(benches);
