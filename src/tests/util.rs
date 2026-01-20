use crate::st::Histogram;

macro_rules! succ {
    ($e:expr) => {
        assert!($e.is_ok());
    }
}

macro_rules! assert_approx_eq {
    ($e:expr, $v:expr, $t:expr) => {
        assert!(($e as f64 - $v as f64).abs() <= $t as f64, "{} !~= {} within {}", $e, $v, $t)
    }
}

pub fn stat_histo() -> Histogram<u64> {
    let mut histogram = Histogram::<u64>::with_low_high_sigvdig(1, 3600 * 1000 * 1000, 3).unwrap();
    for _ in 0..10000 {
        histogram
            .record_value_with_expected_interval(1000, 10000)
            .unwrap();
    }
    histogram
        .record_value_with_expected_interval(100000000, 10000)
        .unwrap();
    histogram
}

pub fn raw_stat_histo() -> Histogram<u64> {
    let mut histogram = Histogram::<u64>::with_low_high_sigvdig(1, 3600 * 1000 * 1000, 3).unwrap();
    for _ in 0..10000 {
        histogram.record_value(1000).unwrap();
    }
    histogram.record_value(100000000).unwrap();
    histogram
}
