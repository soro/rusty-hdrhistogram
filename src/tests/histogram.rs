use crate::core::{Counter, RecordError, SubtractionError};
use crate::st::Histogram;
use crate::tests::consts::*;
use crate::tests::util::*;

fn verify_max_value<T: Counter>(histogram: Histogram<T>) {
    let mut computed_max_value = 0;
    for i in 0..histogram.counts_array_length() {
        if *histogram.get_count_at_index(i).unwrap() > T::zero() {
            computed_max_value = histogram.value_from_index(i);
        }
    }
    computed_max_value = if computed_max_value == 0 {
        0
    } else {
        histogram.highest_equivalent_value(computed_max_value)
    };
    assert_eq!(computed_max_value, histogram.get_max_value());
}

#[test]
fn empty_histogram() {
    let h = Histogram::<u64>::new(SIG_V_DIGITS).unwrap();
    assert_eq!(h.get_min_value(), 0);
    assert_eq!(h.get_max_value(), 0);
    assert_approx_eq!(h.get_mean(), 0.0, 0.00000001);
    assert_approx_eq!(h.get_std_deviation(), 0.0, 0.00000001);
    assert_approx_eq!(h.get_percentile_at_or_below_value(0), 100.0, 0.0000001);
}

#[test]
fn test_record_value() {
    let mut histogram = Histogram::<u64>::with_high_sigvdig(HIGHEST_TRACKABLE, SIG_V_DIGITS).unwrap();
    succ!(histogram.record_value(TEST_VALUE_LEVEL));
    assert_eq!(Some(1), histogram.get_count_at_value(TEST_VALUE_LEVEL));
    assert_eq!(1, histogram.get_total_count());
    verify_max_value(histogram);
}

#[test]
fn record_value_overflow_saturates() {
    let highest = 3600_u64 * 1000 * 1000;
    let mut histogram = Histogram::<u64>::with_high_sigvdig(highest, SIG_V_DIGITS).unwrap();
    succ!(histogram.record_value(highest * 3));
    let last_idx = histogram.last_index();
    assert_eq!(Some(1), histogram.get_count_at_index(last_idx).copied());
}

#[test]
fn record_value_with_expected_interval() {
    let mut histogram = Histogram::<u64>::with_high_sigvdig(HIGHEST_TRACKABLE, SIG_V_DIGITS).unwrap();
    succ!(histogram.record_value_with_expected_interval(TEST_VALUE_LEVEL, TEST_VALUE_LEVEL / 4));
    let mut raw_histogram = Histogram::<u64>::with_high_sigvdig(HIGHEST_TRACKABLE, SIG_V_DIGITS).unwrap();
    succ!(raw_histogram.record_value(TEST_VALUE_LEVEL));

    // should contain extra compensation entries
    assert_eq!(
        Some(1),
        histogram.get_count_at_value((TEST_VALUE_LEVEL * 1) / 4)
    );
    assert_eq!(
        Some(1),
        histogram.get_count_at_value((TEST_VALUE_LEVEL * 2) / 4)
    );
    assert_eq!(
        Some(1),
        histogram.get_count_at_value((TEST_VALUE_LEVEL * 3) / 4)
    );
    assert_eq!(
        Some(1),
        histogram.get_count_at_value((TEST_VALUE_LEVEL * 4) / 4)
    );
    assert_eq!(4, histogram.get_total_count());

    // should not
    assert_eq!(
        Some(0),
        raw_histogram.get_count_at_value((TEST_VALUE_LEVEL * 1) / 4)
    );
    assert_eq!(
        Some(0),
        raw_histogram.get_count_at_value((TEST_VALUE_LEVEL * 2) / 4)
    );
    assert_eq!(
        Some(0),
        raw_histogram.get_count_at_value((TEST_VALUE_LEVEL * 3) / 4)
    );
    assert_eq!(
        Some(1),
        raw_histogram.get_count_at_value((TEST_VALUE_LEVEL * 4) / 4)
    );
    assert_eq!(1, raw_histogram.get_total_count());

    verify_max_value(histogram);
}

#[test]
fn construction_with_large_numbers() {
    let mut histogram = Histogram::<u64>::with_low_high_sigvdig(20000000, 100000000, 5).unwrap();

    succ!(histogram.record_value(100000000));
    succ!(histogram.record_value(20000000));
    succ!(histogram.record_value(30000000));

    assert!(histogram.values_are_equivalent(20000000, histogram.get_value_at_percentile(50.0)));
    assert!(histogram.values_are_equivalent(30000000, histogram.get_value_at_percentile(50.0)));
    assert!(histogram.values_are_equivalent(100000000, histogram.get_value_at_percentile(83.33)));
    assert!(histogram.values_are_equivalent(100000000, histogram.get_value_at_percentile(83.34)));
    assert!(histogram.values_are_equivalent(100000000, histogram.get_value_at_percentile(99.0)));
}

#[test]
fn size_of_equivalent_value_range() {
    let histogram = Histogram::<u64>::with_high_sigvdig(HIGHEST_TRACKABLE, SIG_V_DIGITS).unwrap();
    assert_eq!(
        1,
        histogram.size_of_equivalent_value_range(1),
        "Size of equivalent range for value 1 is 1"
    );
    assert_eq!(
        1,
        histogram.size_of_equivalent_value_range(1025),
        "Size of equivalent range for value 1025 is 1"
    );
    assert_eq!(
        1,
        histogram.size_of_equivalent_value_range(2047),
        "Size of equivalent range for value 2047 is 1"
    );
    assert_eq!(
        2,
        histogram.size_of_equivalent_value_range(2048),
        "Size of equivalent range for value 2048 is 2"
    );
    assert_eq!(
        2,
        histogram.size_of_equivalent_value_range(2500),
        "Size of equivalent range for value 2500 is 2"
    );
    assert_eq!(
        4,
        histogram.size_of_equivalent_value_range(8191),
        "Size of equivalent range for value 8191 is 4"
    );
    assert_eq!(
        8,
        histogram.size_of_equivalent_value_range(8192),
        "Size of equivalent range for value 8192 is 8"
    );
    assert_eq!(
        8,
        histogram.size_of_equivalent_value_range(10000),
        "Size of equivalent range for value 10000 is 8"
    );
    verify_max_value(histogram);
}

#[test]
fn scaled_size_of_equivalent_value_range() {
    let histogram = Histogram::<u64>::with_low_high_sigvdig(1024, HIGHEST_TRACKABLE, SIG_V_DIGITS).unwrap();
    assert_eq!(
        1 * 1024,
        histogram.size_of_equivalent_value_range(1 * 1024),
        "Size of equivalent range for value 1 * 1024 is 1 * 1024"
    );
    assert_eq!(
        2 * 1024,
        histogram.size_of_equivalent_value_range(2500 * 1024),
        "Size of equivalent range for value 2500 * 1024 is 2 * 1024"
    );
    assert_eq!(
        4 * 1024,
        histogram.size_of_equivalent_value_range(8191 * 1024),
        "Size of equivalent range for value 8191 * 1024 is 4 * 1024"
    );
    assert_eq!(
        8 * 1024,
        histogram.size_of_equivalent_value_range(8192 * 1024),
        "Size of equivalent range for value 8192 * 1024 is 8 * 1024"
    );
    assert_eq!(
        8 * 1024,
        histogram.size_of_equivalent_value_range(10000 * 1024),
        "Size of equivalent range for value 10000 * 1024 is 8 * 1024"
    );
    verify_max_value(histogram);
}

#[test]
fn lowest_equivalent_value() {
    let histogram = Histogram::<u64>::with_high_sigvdig(HIGHEST_TRACKABLE, SIG_V_DIGITS).unwrap();
    assert_eq!(
        10000,
        histogram.lowest_equivalent_value(10007),
        "The lowest equivalent value to 10007 is 10000"
    );
    assert_eq!(
        10008,
        histogram.lowest_equivalent_value(10009),
        "The lowest equivalent value to 10009 is 10008"
    );
    verify_max_value(histogram);
}


#[test]
fn scaled_lowest_equivalent_value() {
    let histogram = Histogram::<u64>::with_low_high_sigvdig(1024, HIGHEST_TRACKABLE, SIG_V_DIGITS).unwrap();
    assert_eq!(
        10000 * 1024,
        histogram.lowest_equivalent_value(10007 * 1024),
        "The lowest equivalent value to 10007 * 1024 is 10000 * 1024"
    );
    assert_eq!(
        10008 * 1024,
        histogram.lowest_equivalent_value(10009 * 1024),
        "The lowest equivalent value to 10009 * 1024 is 10008 * 1024"
    );
    verify_max_value(histogram);
}

#[test]
fn equals_with_different_lengths() {
    let mut small = Histogram::<u64>::with_high_sigvdig(2_000, 2).unwrap();
    let mut large = Histogram::<u64>::with_high_sigvdig(2_000_000, 2).unwrap();
    assert_ne!(small.counts_array_length(), large.counts_array_length());

    succ!(small.record_value(1));
    succ!(small.record_value(1000));
    succ!(large.record_value(1));
    succ!(large.record_value(1000));

    assert!(small == large);
    assert!(large == small);
}

#[test]
fn equals_with_different_lengths_detects_mismatch() {
    let mut small = Histogram::<u64>::with_high_sigvdig(2_000, 2).unwrap();
    let mut large = Histogram::<u64>::with_high_sigvdig(2_000_000, 2).unwrap();
    assert_ne!(small.counts_array_length(), large.counts_array_length());

    succ!(small.record_value(1000));
    succ!(large.record_value(1500));

    assert!(small != large);
    assert!(large != small);
}

#[test]
fn highest_equivalent_value() {
    let histogram = Histogram::<u64>::with_low_high_sigvdig(1024, HIGHEST_TRACKABLE, SIG_V_DIGITS).unwrap();
    assert_eq!(
        8183 * 1024 + 1023,
        histogram.highest_equivalent_value(8180 * 1024),
        "The highest equivalent value to 8180 * 1024 is 8183 * 1024 + 1023"
    );
    assert_eq!(
        8191 * 1024 + 1023,
        histogram.highest_equivalent_value(8191 * 1024),
        "The highest equivalent value to 8187 * 1024 is 8191 * 1024 + 1023"
    );
    assert_eq!(
        8199 * 1024 + 1023,
        histogram.highest_equivalent_value(8193 * 1024),
        "The highest equivalent value to 8193 * 1024 is 8199 * 1024 + 1023"
    );
    assert_eq!(
        9999 * 1024 + 1023,
        histogram.highest_equivalent_value(9995 * 1024),
        "The highest equivalent value to 9995 * 1024 is 9999 * 1024 + 1023"
    );
    assert_eq!(
        10007 * 1024 + 1023,
        histogram.highest_equivalent_value(10007 * 1024),
        "The highest equivalent value to 10007 * 1024 is 10007 * 1024 + 1023"
    );
    assert_eq!(
        10015 * 1024 + 1023,
        histogram.highest_equivalent_value(10008 * 1024),
        "The highest equivalent value to 10008 * 1024 is 10015 * 1024 + 1023"
    );
    verify_max_value(histogram);
}

#[test]
fn scaled_highest_equivalent_value() {
    let histogram = Histogram::<u64>::with_high_sigvdig(HIGHEST_TRACKABLE, SIG_V_DIGITS).unwrap();
    assert_eq!(
        8183,
        histogram.highest_equivalent_value(8180),
        "The highest equivalent value to 8180 is 8183"
    );
    assert_eq!(
        8191,
        histogram.highest_equivalent_value(8191),
        "The highest equivalent value to 8187 is 8191"
    );
    assert_eq!(
        8199,
        histogram.highest_equivalent_value(8193),
        "The highest equivalent value to 8193 is 8199"
    );
    assert_eq!(
        9999,
        histogram.highest_equivalent_value(9995),
        "The highest equivalent value to 9995 is 9999"
    );
    assert_eq!(
        10007,
        histogram.highest_equivalent_value(10007),
        "The highest equivalent value to 10007 is 10007"
    );
    assert_eq!(
        10015,
        histogram.highest_equivalent_value(10008),
        "The highest equivalent value to 10008 is 10015"
    );
    verify_max_value(histogram);
}

#[test]
fn median_equivalent_value() {
    let histogram = Histogram::<u64>::with_high_sigvdig(HIGHEST_TRACKABLE, SIG_V_DIGITS).unwrap();
    assert_eq!(
        4,
        histogram.median_equivalent_value(4),
        "The median equivalent value to 4 is 4"
    );
    assert_eq!(
        5,
        histogram.median_equivalent_value(5),
        "The median equivalent value to 5 is 5"
    );
    assert_eq!(
        4001,
        histogram.median_equivalent_value(4000),
        "The median equivalent value to 4000 is 4001"
    );
    assert_eq!(
        8002,
        histogram.median_equivalent_value(8000),
        "The median equivalent value to 8000 is 8002"
    );
    assert_eq!(
        10004,
        histogram.median_equivalent_value(10007),
        "The median equivalent value to 10007 is 10004"
    );
    verify_max_value(histogram);
}

#[test]
fn scaled_median_equivalent_value() {
    let histogram = Histogram::<u64>::with_low_high_sigvdig(1024, HIGHEST_TRACKABLE, SIG_V_DIGITS).unwrap();
    assert_eq!(
        4 * 1024 + 512,
        histogram.median_equivalent_value(4 * 1024),
        "The median equivalent value to 4 * 1024 is 4 * 1024 + 512"
    );
    assert_eq!(
        5 * 1024 + 512,
        histogram.median_equivalent_value(5 * 1024),
        "The median equivalent value to 5 * 1024 is 5 * 1024 + 512"
    );
    assert_eq!(
        4001 * 1024,
        histogram.median_equivalent_value(4000 * 1024),
        "The median equivalent value to 4000 * 1024 is 4001 * 1024"
    );
    assert_eq!(
        8002 * 1024,
        histogram.median_equivalent_value(8000 * 1024),
        "The median equivalent value to 8000 * 1024 is 8002 * 1024"
    );
    assert_eq!(
        10004 * 1024,
        histogram.median_equivalent_value(10007 * 1024),
        "The median equivalent value to 10007 * 1024 is 10004 * 1024"
    );
    verify_max_value(histogram);
}

struct DataHistograms {
    histogram: Histogram<u64>,
    scaled_histogram: Histogram<u64>,
    raw_histogram: Histogram<u64>,
    scaled_raw_histogram: Histogram<u64>,
}

fn build_data_histograms() -> DataHistograms {
    let highest_trackable_value = 3_600_u64 * 1000 * 1000;
    let mut histogram = Histogram::<u64>::with_high_sigvdig(highest_trackable_value, SIG_V_DIGITS).unwrap();
    let mut scaled_histogram =
        Histogram::<u64>::with_low_high_sigvdig(1000, highest_trackable_value * 512, SIG_V_DIGITS).unwrap();
    let mut raw_histogram = Histogram::<u64>::with_high_sigvdig(highest_trackable_value, SIG_V_DIGITS).unwrap();
    let mut scaled_raw_histogram =
        Histogram::<u64>::with_low_high_sigvdig(1000, highest_trackable_value * 512, SIG_V_DIGITS).unwrap();

    for _ in 0..10000 {
        succ!(histogram.record_value_with_expected_interval(1000, 10000));
        succ!(scaled_histogram.record_value_with_expected_interval(1000 * 512, 10000 * 512));
        succ!(raw_histogram.record_value(1000));
        succ!(scaled_raw_histogram.record_value(1000 * 512));
    }
    succ!(histogram.record_value_with_expected_interval(100000000, 10000));
    succ!(scaled_histogram.record_value_with_expected_interval(100000000 * 512, 10000 * 512));
    succ!(raw_histogram.record_value(100000000));
    succ!(scaled_raw_histogram.record_value(100000000 * 512));

    DataHistograms {
        histogram,
        scaled_histogram,
        raw_histogram,
        scaled_raw_histogram,
    }
}

#[test]
fn data_access_scaling_equivalence() {
    let data = build_data_histograms();
    assert_approx_eq!(
        data.histogram.get_mean() * 512.0,
        data.scaled_histogram.get_mean(),
        data.scaled_histogram.get_mean() * 0.000001
    );
    assert_eq!(data.histogram.get_total_count(), data.scaled_histogram.get_total_count());
    assert_eq!(
        data.scaled_histogram.highest_equivalent_value(data.histogram.get_value_at_percentile(99.0) * 512),
        data.scaled_histogram.highest_equivalent_value(data.scaled_histogram.get_value_at_percentile(99.0))
    );
    assert_eq!(
        data.scaled_histogram.highest_equivalent_value(data.histogram.get_max_value() * 512),
        data.scaled_histogram.get_max_value()
    );
}

#[test]
fn data_access_get_total_count_and_min_max() {
    let data = build_data_histograms();
    assert_eq!(10001, data.raw_histogram.get_total_count());
    assert_eq!(20000, data.histogram.get_total_count());
    assert!(data.histogram.values_are_equivalent(100000000, data.histogram.get_max_value()));
    assert!(data.histogram.values_are_equivalent(1000, data.histogram.get_min_value()));
}

#[test]
fn data_access_raw_and_corrected_stats() {
    let data = build_data_histograms();
    let expected_raw_mean: f64 = ((10000.0 * 1000.0) + (1.0 * 100000000.0)) / 10001.0;
    let expected_mean: f64 = (1000.0 + 50000000.0) / 2.0;
    assert_approx_eq!(expected_raw_mean, data.raw_histogram.get_mean(), expected_raw_mean * 0.001);
    assert_approx_eq!(expected_mean, data.histogram.get_mean(), expected_mean * 0.001);

    let expected_raw_stddev: f64 = (((10000.0 * (1000.0 - expected_raw_mean).powi(2))
        + (100000000.0 - expected_raw_mean).powi(2))
        / 10001.0)
        .sqrt();
    let mut expected_square_deviation_sum: f64 = 10000.0 * (1000.0 - expected_mean).powi(2);
    let mut curr_val: f64 = 10000.0;
    while curr_val <= 100000000.0 {
        expected_square_deviation_sum += (curr_val - expected_mean).powi(2);
        curr_val += 10000.0;
    }
    let expected_std_deviation: f64 = (expected_square_deviation_sum / 20000.0).sqrt();
    assert_approx_eq!(
        expected_raw_stddev,
        data.raw_histogram.get_std_deviation(),
        expected_raw_stddev * 0.001
    );
    assert_approx_eq!(
        expected_std_deviation,
        data.histogram.get_std_deviation(),
        expected_std_deviation * 0.001
    );
}

#[test]
fn data_access_get_value_at_percentile() {
    let data = build_data_histograms();
    assert_approx_eq!(
        1000.0,
        data.raw_histogram.get_value_at_percentile(30.0) as f64,
        1000.0 * 0.001
    );
    assert_approx_eq!(
        1000.0,
        data.raw_histogram.get_value_at_percentile(99.0) as f64,
        1000.0 * 0.001
    );
    assert_approx_eq!(
        1000.0,
        data.raw_histogram.get_value_at_percentile(99.99) as f64,
        1000.0 * 0.001
    );
    assert_approx_eq!(
        100000000.0,
        data.raw_histogram.get_value_at_percentile(99.999) as f64,
        100000000.0 * 0.001
    );
    assert_approx_eq!(
        100000000.0,
        data.raw_histogram.get_value_at_percentile(100.0) as f64,
        100000000.0 * 0.001
    );

    assert_approx_eq!(
        1000.0,
        data.histogram.get_value_at_percentile(30.0) as f64,
        1000.0 * 0.001
    );
    assert_approx_eq!(
        1000.0,
        data.histogram.get_value_at_percentile(50.0) as f64,
        1000.0 * 0.001
    );
    assert_approx_eq!(
        50000000.0,
        data.histogram.get_value_at_percentile(75.0) as f64,
        50000000.0 * 0.001
    );
    assert_approx_eq!(
        80000000.0,
        data.histogram.get_value_at_percentile(90.0) as f64,
        80000000.0 * 0.001
    );
    assert_approx_eq!(
        98000000.0,
        data.histogram.get_value_at_percentile(99.0) as f64,
        98000000.0 * 0.001
    );
    assert_approx_eq!(
        100000000.0,
        data.histogram.get_value_at_percentile(99.999) as f64,
        100000000.0 * 0.001
    );
    assert_approx_eq!(
        100000000.0,
        data.histogram.get_value_at_percentile(100.0) as f64,
        100000000.0 * 0.001
    );
}

#[test]
fn get_value_at_percentile_examples() {
    let mut histogram = Histogram::<u64>::with_high_sigvdig(3600 * 1000 * 1000, SIG_V_DIGITS).unwrap();
    succ!(histogram.record_value(1));
    succ!(histogram.record_value(2));
    assert_eq!(1, histogram.get_value_at_percentile(50.0));
    assert_eq!(1, histogram.get_value_at_percentile(50.00000000000001));
    assert_eq!(2, histogram.get_value_at_percentile(50.0000000000001));

    succ!(histogram.record_value(2));
    succ!(histogram.record_value(2));
    succ!(histogram.record_value(2));
    assert_eq!(2, histogram.get_value_at_percentile(25.0));
    assert_eq!(2, histogram.get_value_at_percentile(30.0));
}

#[test]
fn various_stats() {
    let histogram = stat_histo();

    assert_eq!(20000, histogram.get_total_count(), "wrong total count");

    assert!(
        histogram.values_are_equivalent(100000000, histogram.get_max_value()),
        "wrong max value"
    );

    // check mean
    let expected_mean = (1000.0 + 50000000.0) / 2.0;
    assert_approx_eq!(expected_mean, histogram.get_mean(), expected_mean * 0.001);

    // check std deviation
    let mut expected_square_deviation_sum = 10000.0 * f64::powi(1000.0 - expected_mean, 2);
    let mut curr_val = 10000;
    while curr_val < 100000001 {
        expected_square_deviation_sum += f64::powi(curr_val as f64 - expected_mean, 2);
        curr_val += 10000;
    }
    let expected_std_deviation = (expected_square_deviation_sum / histogram.get_total_count() as f64).sqrt();
    assert_approx_eq!(
        expected_std_deviation,
        histogram.get_std_deviation(),
        expected_std_deviation * 0.001
    );
}

#[test]
fn value_at_percentile() {
    let histogram = stat_histo();

    assert_approx_eq!(
        1000.0,
        histogram.get_value_at_percentile(30.0),
        1000.0 * 0.001
    );
    assert_approx_eq!(
        1000.0,
        histogram.get_value_at_percentile(50.0),
        1000.0 * 0.001
    );
    assert_approx_eq!(
        50000000.0,
        histogram.get_value_at_percentile(75.0),
        50000000.0 * 0.001
    );
    assert_approx_eq!(
        80000000.0,
        histogram.get_value_at_percentile(90.0),
        80000000.0 * 0.001
    );
    assert_approx_eq!(
        98000000.0,
        histogram.get_value_at_percentile(99.0),
        98000000.0 * 0.001
    );
    assert_approx_eq!(
        100000000.0,
        histogram.get_value_at_percentile(99.999),
        100000000.0 * 0.001
    );
    assert_approx_eq!(
        100000000.0,
        histogram.get_value_at_percentile(100.0),
        100000000.0 * 0.001
    );
}

#[test]
fn get_value_at_percentile_for_large_histogram() {
    let largest_value = 1000000000000;
    let mut h = Histogram::<u64>::with_high_sigvdig(largest_value, 5).unwrap();

    succ!(h.record_value(largest_value));

    assert!(h.get_value_at_percentile(100.0) > 0);
}

#[test]
fn test_get_percentile_at_or_below_value() {
    let histogram = stat_histo();
    assert_approx_eq!(
        50.0,
        histogram.get_percentile_at_or_below_value(5000),
        0.0001
    );
    assert_approx_eq!(
        100.0,
        histogram.get_percentile_at_or_below_value(100000000),
        0.0001
    );
}


#[test]
fn reset() {
    let mut histogram = Histogram::<u64>::with_high_sigvdig(HIGHEST_TRACKABLE, SIG_V_DIGITS).unwrap();
    succ!(histogram.record_value(TEST_VALUE_LEVEL));
    histogram.reset();
    assert_eq!(Some(0), histogram.get_count_at_value(TEST_VALUE_LEVEL));
    assert_eq!(0, histogram.get_total_count());
    verify_max_value(histogram);
}

#[test]
fn value_at_percentile_matches_percentile() {
    let lengths = vec![1, 5, 10, 50, 100, 500, 1000, 5000, 10000, 50000];

    let mut histogram = Histogram::<u64>::with_low_high_sigvdig(1, 1 << 63, 3).unwrap();

    for length in lengths.iter() {
        histogram.reset();
        for value in 1..=*length {
            succ!(histogram.record_value(value));
        }
        for value in 1..=*length {
            let calculated_percentile = 100.0 * value as f64 / *length as f64;
            let lookup_value = histogram.get_value_at_percentile(calculated_percentile);
            assert!(
                histogram.values_are_equivalent(value, lookup_value),
                "length: {} value: {} calculatedPercentile: {} getValueAtPercentile({}) = {:.5} [should be {}]",
                *length,
                value,
                calculated_percentile,
                calculated_percentile,
                lookup_value,
                value
            );
        }
        assert!(true);
    }
}

#[test]
fn value_at_percentile_matches_percentile_iter() {
    let mut histogram = Histogram::<u64>::with_low_high_sigvdig(1, HIGHEST_TRACKABLE, 3).unwrap();
    let lengths = vec![1, 5, 10, 50, 100, 500, 1000, 5000, 10000, 50000, 100000];

    for length in lengths.iter() {
        histogram.reset();
        for value in 1..=*length {
            succ!(histogram.record_value(value));
        }

        let percentile_ticks_per_half_distance = 1000;
        for v in histogram.percentiles(percentile_ticks_per_half_distance) {
            let calculated_value = histogram.get_value_at_percentile(v.percentile);
            let iter_value = v.value_iterated_to;
            assert!(
                histogram.values_are_equivalent(calculated_value, iter_value),
                "length: {} percentile: {} calculatedValue: {} iterValue: {} [should be {}]",
                length,
                v.percentile,
                calculated_value,
                iter_value,
                calculated_value
            );
        }
    }
}

#[test]
fn add_histograms() {
    let highest = 3_600_u64 * 1000 * 1000;
    let mut histogram = Histogram::<u64>::with_high_sigvdig(highest, SIG_V_DIGITS).unwrap();
    let mut other = Histogram::<u64>::with_high_sigvdig(highest, SIG_V_DIGITS).unwrap();
    succ!(histogram.record_value(TEST_VALUE_LEVEL));
    succ!(histogram.record_value(TEST_VALUE_LEVEL * 1000));
    succ!(other.record_value(TEST_VALUE_LEVEL));
    succ!(other.record_value(TEST_VALUE_LEVEL * 1000));
    succ!(histogram.add(&other));
    assert_eq!(Some(2), histogram.get_count_at_value(TEST_VALUE_LEVEL));
    assert_eq!(Some(2), histogram.get_count_at_value(TEST_VALUE_LEVEL * 1000));
    assert_eq!(4, histogram.get_total_count());

    let mut bigger_other = Histogram::<u64>::with_high_sigvdig(highest * 2, SIG_V_DIGITS).unwrap();
    succ!(bigger_other.record_value(TEST_VALUE_LEVEL));
    succ!(bigger_other.record_value(TEST_VALUE_LEVEL * 1000));
    succ!(bigger_other.record_value(highest * 2));
    succ!(bigger_other.add(&histogram));
    assert_eq!(Some(3), bigger_other.get_count_at_value(TEST_VALUE_LEVEL));
    assert_eq!(Some(3), bigger_other.get_count_at_value(TEST_VALUE_LEVEL * 1000));
    assert_eq!(Some(1), bigger_other.get_count_at_value(highest * 2));
    assert_eq!(7, bigger_other.get_total_count());

    assert!(matches!(
        histogram.add(&bigger_other),
        Err(RecordError::ValueOutOfRangeResizeDisabled)
    ));

    verify_max_value(histogram);
    verify_max_value(other);
    verify_max_value(bigger_other);
}

#[test]
fn subtract_after_add() {
    let highest = 3_600_u64 * 1000 * 1000;
    let mut histogram = Histogram::<u64>::with_high_sigvdig(highest, SIG_V_DIGITS).unwrap();
    let mut other = Histogram::<u64>::with_high_sigvdig(highest, SIG_V_DIGITS).unwrap();
    succ!(histogram.record_value(TEST_VALUE_LEVEL));
    succ!(histogram.record_value(TEST_VALUE_LEVEL * 1000));
    succ!(other.record_value(TEST_VALUE_LEVEL));
    succ!(other.record_value(TEST_VALUE_LEVEL * 1000));
    succ!(histogram.add(&other));
    succ!(histogram.add(&other));
    succ!(histogram.subtract(&other));
    assert_eq!(Some(2), histogram.get_count_at_value(TEST_VALUE_LEVEL));
    assert_eq!(Some(2), histogram.get_count_at_value(TEST_VALUE_LEVEL * 1000));
    assert_eq!(4, histogram.get_total_count());
    verify_max_value(histogram);
    verify_max_value(other);
}

#[test]
fn subtract_to_zero_counts() {
    let highest = 3_600_u64 * 1000 * 1000;
    let mut histogram = Histogram::<u64>::with_high_sigvdig(highest, SIG_V_DIGITS).unwrap();
    let mut other = Histogram::<u64>::with_high_sigvdig(highest, SIG_V_DIGITS).unwrap();
    succ!(histogram.record_value(TEST_VALUE_LEVEL));
    succ!(histogram.record_value(TEST_VALUE_LEVEL * 1000));
    succ!(other.record_value(TEST_VALUE_LEVEL));
    succ!(other.record_value(TEST_VALUE_LEVEL * 1000));
    succ!(histogram.subtract(&other));
    assert_eq!(Some(0), histogram.get_count_at_value(TEST_VALUE_LEVEL));
    assert_eq!(Some(0), histogram.get_count_at_value(TEST_VALUE_LEVEL * 1000));
    assert_eq!(0, histogram.get_total_count());
    verify_max_value(histogram);
}

#[test]
fn subtract_to_negative_counts_throws() {
    let highest = 3_600_u64 * 1000 * 1000;
    let mut histogram = Histogram::<u64>::with_high_sigvdig(highest, SIG_V_DIGITS).unwrap();
    let mut other = Histogram::<u64>::with_high_sigvdig(highest, SIG_V_DIGITS).unwrap();
    succ!(histogram.record_value(TEST_VALUE_LEVEL));
    succ!(histogram.record_value(TEST_VALUE_LEVEL * 1000));
    succ!(other.record_value_with_count(TEST_VALUE_LEVEL, 2));
    succ!(other.record_value_with_count(TEST_VALUE_LEVEL * 1000, 2));

    assert!(matches!(
        histogram.subtract(&other),
        Err(SubtractionError::CountExceededAtValue)
    ));
    assert_eq!(Some(1), histogram.get_count_at_value(TEST_VALUE_LEVEL));
    assert_eq!(Some(1), histogram.get_count_at_value(TEST_VALUE_LEVEL * 1000));
    verify_max_value(histogram);
    verify_max_value(other);
}

#[test]
fn subtract_subtrahend_values_outside_range_throws() {
    let highest = 3_600_u64 * 1000 * 1000;
    let mut histogram = Histogram::<u64>::with_high_sigvdig(highest, SIG_V_DIGITS).unwrap();
    succ!(histogram.record_value(TEST_VALUE_LEVEL));
    succ!(histogram.record_value(TEST_VALUE_LEVEL * 1000));

    let mut bigger_other = Histogram::<u64>::with_high_sigvdig(highest * 2, SIG_V_DIGITS).unwrap();
    succ!(bigger_other.record_value(TEST_VALUE_LEVEL));
    succ!(bigger_other.record_value(TEST_VALUE_LEVEL * 1000));
    succ!(bigger_other.record_value(highest * 2));

    assert!(matches!(
        histogram.subtract(&bigger_other),
        Err(SubtractionError::ValueOutOfRange)
    ));
    verify_max_value(histogram);
    verify_max_value(bigger_other);
}

#[test]
fn subtract_subtrahend_values_inside_range_works() {
    let highest = 3_600_u64 * 1000 * 1000;
    let mut histogram = Histogram::<u64>::with_high_sigvdig(highest, SIG_V_DIGITS).unwrap();
    succ!(histogram.record_value(TEST_VALUE_LEVEL));
    succ!(histogram.record_value(TEST_VALUE_LEVEL * 1000));

    let mut bigger_other = Histogram::<u64>::with_high_sigvdig(highest * 2, SIG_V_DIGITS).unwrap();
    succ!(bigger_other.record_value_with_count(TEST_VALUE_LEVEL, 4));
    succ!(bigger_other.record_value_with_count(TEST_VALUE_LEVEL * 1000, 4));
    succ!(bigger_other.record_value_with_count(highest * 2, 4));

    succ!(bigger_other.subtract(&histogram));
    assert_eq!(Some(3), bigger_other.get_count_at_value(TEST_VALUE_LEVEL));
    assert_eq!(Some(3), bigger_other.get_count_at_value(TEST_VALUE_LEVEL * 1000));
    assert_eq!(Some(4), bigger_other.get_count_at_value(highest * 2));
    assert_eq!(10, bigger_other.get_total_count());
    verify_max_value(histogram);
    verify_max_value(bigger_other);
}

#[test]
fn histogram_auto_resize_edges() {
    let mut histogram = Histogram::<u64>::new(SIG_V_DIGITS).unwrap();
    histogram.set_auto_resize(true);
    succ!(histogram.record_value((1_u64 << 62) - 1));
    let settings = histogram.settings();
    assert_eq!(52, settings.bucket_count);
    assert_eq!(54272, settings.counts_array_length);
    succ!(histogram.record_value(i64::MAX as u64));
    let settings = histogram.settings();
    assert_eq!(53, settings.bucket_count);
    assert_eq!(55296, settings.counts_array_length);
}

#[test]
fn histogram_equals_after_resize() {
    let mut histogram = Histogram::<u64>::new(SIG_V_DIGITS).unwrap();
    histogram.set_auto_resize(true);
    succ!(histogram.record_value((1_u64 << 62) - 1));
    succ!(histogram.record_value(i64::MAX as u64));
    histogram.reset();
    succ!(histogram.record_value((1_u64 << 62) - 1));

    let mut histogram1 = Histogram::<u64>::new(SIG_V_DIGITS).unwrap();
    histogram1.set_auto_resize(true);
    succ!(histogram1.record_value((1_u64 << 62) - 1));
    assert!(histogram.equals(&histogram1));
}

#[test]
fn histogram_auto_resize_across_range() {
    let mut histogram = Histogram::<u64>::new(SIG_V_DIGITS).unwrap();
    histogram.set_auto_resize(true);
    for i in 0..63 {
        succ!(histogram.record_value(1_u64 << i));
    }
    let settings = histogram.settings();
    assert_eq!(53, settings.bucket_count);
    assert_eq!(55296, settings.counts_array_length);
}

fn populate_shift_histogram(histogram: &mut Histogram<u64>, base_shift: u32, extra_shift: u32) {
    succ!(histogram.record_value_with_count(0, 500));
    let shift = base_shift + extra_shift;
    for value in [2_u64, 4, 5, 511, 512, 1023, 1024, 1025] {
        succ!(histogram.record_value(value << shift));
    }
}

#[test]
fn histogram_shift_lowest_bucket() {
    let highest = 3_600_u64 * 1000 * 1000;
    for shift_amount in 0..10 {
        let mut histogram = Histogram::<u64>::with_high_sigvdig(highest, SIG_V_DIGITS).unwrap();
        populate_shift_histogram(&mut histogram, 0, 0);

        let mut expected = Histogram::<u64>::with_high_sigvdig(highest, SIG_V_DIGITS).unwrap();
        populate_shift_histogram(&mut expected, 0, shift_amount);

        succ!(histogram.shift_values_left(shift_amount));
        assert!(histogram.equals(&expected));
    }
}

#[test]
fn histogram_shift_non_lowest_bucket() {
    let highest = 3_600_u64 * 1000 * 1000;
    for shift_amount in 0..10 {
        let mut histogram = Histogram::<u64>::with_high_sigvdig(highest, SIG_V_DIGITS).unwrap();
        populate_shift_histogram(&mut histogram, 10, 0);

        let mut expected = Histogram::<u64>::with_high_sigvdig(highest, SIG_V_DIGITS).unwrap();
        populate_shift_histogram(&mut expected, 10, shift_amount);

        let mut original = Histogram::<u64>::with_high_sigvdig(highest, SIG_V_DIGITS).unwrap();
        populate_shift_histogram(&mut original, 10, 0);

        succ!(histogram.shift_values_left(shift_amount));
        assert!(histogram.equals(&expected));

        succ!(histogram.shift_values_right(shift_amount));
        assert!(histogram.equals(&original));
    }
}
