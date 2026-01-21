use crate::core::Counter;
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
    assert_eq!(Some(1), histogram.get_count_at_index(last_idx));
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
