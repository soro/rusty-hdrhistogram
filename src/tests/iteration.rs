use crate::tests::util::*;
use crate::IterationError;
use std::num::{NonZeroU32, NonZeroU64};

// The same errors must protect ordinary histograms, frozen snapshots,
// and captured live views, including reset before any state is changed.
macro_rules! check_logarithmic_parameters {
    ($histogram:expr, $width:expr, $invalid:expr) => {{
        let histogram = &$histogram;
        let mut reference = histogram.logarithmic_bucket_values($width, 2.0).unwrap();
        let first = reference.try_next().unwrap();
        let second = reference.try_next().unwrap();
        assert!(first.is_some() && second.is_some());
        for (width, base, error) in $invalid {
            assert_eq!(Some(error), histogram.logarithmic_bucket_values(width, base).err());
            let mut iterator = histogram.logarithmic_bucket_values($width, 2.0).unwrap();
            assert_eq!(first, iterator.try_next().unwrap());
            assert_eq!(Err(error), iterator.reset(width, base));
            assert_eq!(second, iterator.try_next().unwrap());
            iterator.reset($width, 2.0).unwrap();
            assert_eq!(first, iterator.try_next().unwrap());
        }
    }};
}

#[test]
fn logarithmic_parameters_are_validated_at_construction_and_reset() {
    let invalid = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, -0.0, 0.0, 0.5, 1.0]
        .map(|base| (NonZeroU64::MIN, base, IterationError::InvalidLogBase));
    let mut histogram = crate::Histogram::builder().significant_digits(0).build().unwrap();
    let mut concurrent = crate::concurrent::ResizableConcurrentHistogram::builder()
        .significant_digits(0)
        .build()
        .unwrap();
    for value in [0, 4, 16] {
        histogram.record_value(value).unwrap();
        concurrent.record_value(value).unwrap();
    }
    check_logarithmic_parameters!(histogram, NonZeroU64::MIN, invalid);
    check_logarithmic_parameters!(concurrent.as_snapshot(), NonZeroU64::MIN, invalid);
    check_logarithmic_parameters!(concurrent.read_view(), NonZeroU64::MIN, invalid);
}

macro_rules! check_double_linear_parameters {
    ($histogram:expr) => {{
        let histogram = &$histogram;
        let mut reference = histogram.linear_bucket_values(1.0).unwrap();
        let first = reference.try_next().unwrap();
        let second = reference.try_next().unwrap();
        assert!(first.is_some() && second.is_some());
        for width in [0.0, -0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX] {
            assert_eq!(
                Some(IterationError::InvalidBucketWidth),
                histogram.linear_bucket_values(width).err()
            );
            let mut iterator = histogram.linear_bucket_values(1.0).unwrap();
            assert_eq!(first, iterator.try_next().unwrap());
            assert_eq!(Err(IterationError::InvalidBucketWidth), iterator.reset(width));
            assert_eq!(second, iterator.try_next().unwrap());
            iterator.reset(1.0).unwrap();
            assert_eq!(first, iterator.try_next().unwrap());
        }
        // A positive sub-unit width retains the existing round-up-to-one behavior.
        assert!(histogram
            .linear_bucket_values(f64::from_bits(1))
            .unwrap()
            .try_next()
            .unwrap()
            .is_some());
    }};
}

#[test]
fn double_linear_parameters_are_validated_at_construction_and_reset() {
    let mut histogram = crate::DoubleHistogram::builder().significant_digits(0).build().unwrap();
    let concurrent = crate::concurrent::ConcurrentDoubleHistogram::builder()
        .significant_digits(0)
        .build()
        .unwrap();
    for value in [0.0, 4.0, 16.0] {
        histogram.record_value(value).unwrap();
        concurrent.record_value(value).unwrap();
    }
    check_double_linear_parameters!(histogram);
    check_double_linear_parameters!(concurrent.read_view());
    let recorder = crate::DoubleRecorder::from_histogram(concurrent);
    let sample = recorder.begin_interval_sample();
    check_double_linear_parameters!(sample);
    check_double_linear_parameters!(sample.snapshot());
}

#[test]
fn double_logarithmic_parameters_are_validated_at_construction_and_reset() {
    let mut invalid = vec![];
    for width in [0.0, -0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX] {
        invalid.push((width, 2.0, IterationError::InvalidBucketWidth));
    }
    for base in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, -0.0, 0.0, 0.5, 1.0] {
        invalid.push((1.0, base, IterationError::InvalidLogBase));
    }
    let mut histogram = crate::DoubleHistogram::builder().significant_digits(0).build().unwrap();
    let concurrent = crate::concurrent::ConcurrentDoubleHistogram::builder()
        .significant_digits(0)
        .build()
        .unwrap();
    for value in [0.0, 4.0, 16.0] {
        histogram.record_value(value).unwrap();
        concurrent.record_value(value).unwrap();
    }
    check_logarithmic_parameters!(histogram, 1.0, invalid.iter().copied());
    check_logarithmic_parameters!(concurrent.read_view(), 1.0, invalid.iter().copied());
    let recorder = crate::DoubleRecorder::from_histogram(concurrent);
    let sample = recorder.begin_interval_sample();
    check_logarithmic_parameters!(sample.snapshot(), 1.0, invalid.iter().copied());
}

#[test]
fn percentiles() {
    let histogram = stat_histo();
    for value in histogram.percentiles(NonZeroU32::new(5).unwrap()) {
        let value_at_pctl = histogram.get_value_at_percentile(value.percentile);
        assert_eq!(value.value_iterated_to, histogram.highest_equivalent_value(value_at_pctl));
    }
}

#[test]
fn percentile_tick_counts_retain_the_full_unsigned_range() {
    let mut histogram = crate::Histogram::builder().build().unwrap();
    histogram.record_value(1).unwrap();
    histogram.record_value(2).unwrap();
    let ticks = NonZeroU32::new(u32::MAX).unwrap();
    let mut iterator = histogram.percentiles(ticks);
    assert_eq!(0.0, iterator.next().unwrap().percentile_level_iterated_to);
    assert_eq!(
        100.0 / (2 * u64::from(u32::MAX)) as f64,
        iterator.next().unwrap().percentile_level_iterated_to
    );
    iterator.reset(NonZeroU32::MIN);
    assert_eq!(2, iterator.last().unwrap().total_count_to_this_value);
}

#[test]
fn linear_bucket_values() {
    let mut index = 0;
    let histogram = stat_histo();
    let raw_histogram = raw_stat_histo();

    for value in raw_histogram.linear_bucket_values(NonZeroU64::new(100000).unwrap()) {
        let count_added_in_this_bucket = value.count_added_in_this_iteration_step;
        if index == 0 {
            assert_eq!(
                10000, count_added_in_this_bucket,
                "Raw Linear 100 msec bucket # 0 added a count of 10000"
            );
        } else if index == 999 {
            assert_eq!(1, count_added_in_this_bucket, "Raw Linear 100 msec bucket # 999 added a count of 1");
        } else {
            assert_eq!(
                0, count_added_in_this_bucket,
                "Raw Linear 100 msec bucket # {} added a count of 0",
                index
            );
        }
        index += 1;
    }
    assert_eq!(1000, index);

    index = 0;
    let mut total_added_counts = 0;

    for value in histogram.linear_bucket_values(NonZeroU64::new(10000).unwrap()) {
        let count_added_in_this_bucket = value.count_added_in_this_iteration_step;
        if index == 0 {
            assert_eq!(
                10000, count_added_in_this_bucket,
                "Linear 1 sec bucket # 0 [{}..{}] added a count of 10000",
                value.value_iterated_from, value.value_iterated_to
            );
        }
        total_added_counts += value.count_added_in_this_iteration_step;
        index += 1;
    }

    assert_eq!(
        10000, index,
        "There should be 10000 linear buckets of size 10000 usec between 0 and 100 sec."
    );
    assert_eq!(20000, total_added_counts, "Total added counts should be 20000");

    index = 0;
    total_added_counts = 0;

    for value in histogram.linear_bucket_values(NonZeroU64::new(1000).unwrap()) {
        let count_added_in_this_bucket = value.count_added_in_this_iteration_step;
        if index == 1 {
            assert_eq!(
                10000, count_added_in_this_bucket,
                "Linear 1 sec bucket # 0 [{}..{}] added a count of 10000",
                value.value_iterated_from, value.value_iterated_to
            );
        }
        total_added_counts += value.count_added_in_this_iteration_step;
        index += 1;
    }

    assert_eq!(
        100007, index,
        "There should be 100007 linear buckets of size 1000 usec between 0 and 100 sec."
    );
    assert_eq!(20000, total_added_counts, "Total added counts should be 20000");
}

#[test]
fn logarithmic_bucket_values() {
    let histogram = stat_histo();
    let raw_histogram = raw_stat_histo();

    let mut index = 0;

    for value in raw_histogram
        .logarithmic_bucket_values(NonZeroU64::new(10000).unwrap(), 2.0)
        .unwrap()
    {
        let count_added_in_this_bucket = value.count_added_in_this_iteration_step;
        if index == 0 {
            assert_eq!(
                10000, count_added_in_this_bucket,
                "Raw Logarithmic 10 msec bucket # 0 added a count of 10000"
            );
        } else if index == 14 {
            assert_eq!(
                1, count_added_in_this_bucket,
                "Raw Logarithmic 10 msec bucket # 14 added a count of 1"
            );
        } else {
            assert_eq!(
                0, count_added_in_this_bucket,
                "Raw Logarithmic 100 msec bucket # {} added a count of 0",
                index
            );
        }
        index += 1;
    }
    assert_eq!(14, index - 1);

    index = 0;
    let mut total_added_counts = 0;

    for value in histogram.logarithmic_bucket_values(NonZeroU64::new(10000).unwrap(), 2.0).unwrap() {
        let count_added_in_this_bucket = value.count_added_in_this_iteration_step;
        if index == 0 {
            assert_eq!(
                10000, count_added_in_this_bucket,
                "Logarithmic 10 msec bucket # 0 [{}..{}] added a count of 10000",
                value.value_iterated_from, value.value_iterated_to
            );
        }
        total_added_counts += value.count_added_in_this_iteration_step;
        index += 1;
    }

    assert_eq!(
        14,
        index - 1,
        "There should be 14 Logarithmic buckets of size 10000 usec between 0 and 100 sec."
    );
    assert_eq!(20000, total_added_counts, "Total added counts should be 20000");
}

#[test]
fn recorded_values() {
    let histogram = stat_histo();
    let raw_histogram = raw_stat_histo();

    let mut index = 0;

    for value in raw_histogram.recorded_values() {
        let count_added_in_this_bucket = value.count_added_in_this_iteration_step;
        if index == 0 {
            assert_eq!(
                10000, count_added_in_this_bucket,
                "Raw recorded value bucket # 0 added a count of 10000"
            );
        } else {
            assert_eq!(
                1, count_added_in_this_bucket,
                "Raw recorded value bucket # {} added a count of 1",
                index
            );
        }
        index += 1;
    }
    assert_eq!(2, index);

    index = 0;
    let mut total_added_counts = 0;
    for value in histogram.recorded_values() {
        let count_added_in_this_bucket = value.count_added_in_this_iteration_step;
        if index == 0 {
            assert_eq!(
                10000, count_added_in_this_bucket,
                "Recorded bucket # 0 [{}..{}] added a count of 10000",
                value.value_iterated_from, value.value_iterated_to
            );
        }
        assert!(
            value.count_at_value_iterated_to != 0,
            "The count in recorded bucket #{} is not 0",
            index
        );
        assert_eq!(
            value.count_at_value_iterated_to, value.count_added_in_this_iteration_step,
            "The count in recorded bucket # {} is exactly the amount added since the last iteration",
            index
        );
        total_added_counts += value.count_added_in_this_iteration_step;
        index += 1;
    }
    assert_eq!(20000, total_added_counts, "Total added counts should be 20000");
}

#[test]
fn all_values() {
    let histogram = stat_histo();
    let raw_histogram = raw_stat_histo();

    let mut index = 0;
    #[allow(unused_assignments)]
    let mut latest_value_at_index = 0;
    let mut total_count_to_this_point = 0;
    let mut total_value_to_this_point = 0;

    for value in raw_histogram.all_values() {
        let count_added_in_this_bucket = value.count_added_in_this_iteration_step;
        if index == 1000 {
            assert_eq!(10000, count_added_in_this_bucket, "Raw allValues bucket # 0 added a count of 10000");
        } else if histogram.values_are_equivalent(value.value_iterated_to, 100000000) {
            assert_eq!(
                1, count_added_in_this_bucket,
                "Raw allValues value bucket # {} added a count of 1",
                index
            );
        } else {
            assert_eq!(
                0, count_added_in_this_bucket,
                "Raw allValues value bucket # {} added a count of 0",
                index
            );
        }
        latest_value_at_index = value.value_iterated_to;
        total_count_to_this_point += value.count_at_value_iterated_to;
        assert_eq!(
            total_count_to_this_point, value.total_count_to_this_value,
            "total Count should match"
        );
        total_value_to_this_point += value.count_at_value_iterated_to * latest_value_at_index;
        assert_eq!(
            total_value_to_this_point, value.total_value_to_this_value,
            "total Value should match"
        );
        index += 1;
    }
    assert_eq!(histogram.counts_array_length(), index, "index should be equal to countsArrayLength");

    index = 0;
    let mut total_added_counts = 0;
    for value in histogram.all_values() {
        let count_added_in_this_bucket = value.count_added_in_this_iteration_step;
        if index == 1000 {
            assert_eq!(
                10000, count_added_in_this_bucket,
                "AllValues bucket # 0 [{}..{}] added a count of 10000",
                value.value_iterated_from, value.value_iterated_to
            );
        }
        assert_eq!(
            value.count_at_value_iterated_to, value.count_added_in_this_iteration_step,
            "The count in AllValues bucket # {} is exactly the amount added since the last iteration",
            index
        );
        total_added_counts += value.count_added_in_this_iteration_step;
        assert!(
            histogram.values_are_equivalent(histogram.value_from_index(index), value.value_iterated_to),
            "valueFromIndex(index) should be equal to getValueIteratedTo()"
        );
        index += 1;
    }
    assert_eq!(histogram.counts_array_length(), index, "index should be equal to countsArrayLength");
    assert_eq!(20000, total_added_counts, "Total added counts should be 20000");
}
