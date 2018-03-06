use tests::util::*;

#[test]
fn percentiles() {
    let histogram = stat_histo();
    for value in histogram.percentiles(5) {
        let value_at_pctl = histogram.get_value_at_percentile(value.percentile);
        assert_eq!(
            value.value_iterated_to,
            histogram.highest_equivalent_value(value_at_pctl)
        );
    }
}

#[test]
fn linear_bucket_values() {
    let mut index = 0;
    let histogram = stat_histo();
    let raw_histogram = raw_stat_histo();

    for value in raw_histogram.linear_bucket_values(100000) {
        let count_added_in_this_bucket = value.count_added_in_this_iteration_step;
        if index == 0 {
            assert_eq!(
                10000,
                count_added_in_this_bucket,
                "Raw Linear 100 msec bucket # 0 added a count of 10000"
            );
        } else if index == 999 {
            assert_eq!(
                1,
                count_added_in_this_bucket,
                "Raw Linear 100 msec bucket # 999 added a count of 1"
            );
        } else {
            assert_eq!(
                0,
                count_added_in_this_bucket,
                "Raw Linear 100 msec bucket # {} added a count of 0",
                index
            );
        }
        index += 1;
    }
    assert_eq!(1000, index);

    index = 0;
    let mut total_added_counts = 0;

    for value in histogram.linear_bucket_values(10000) {
        let count_added_in_this_bucket = value.count_added_in_this_iteration_step;
        if index == 0 {
            assert_eq!(
                10000,
                count_added_in_this_bucket,
                "Linear 1 sec bucket # 0 [{}..{}] added a count of 10000",
                value.value_iterated_from,
                value.value_iterated_to
            );
        }
        total_added_counts += value.count_added_in_this_iteration_step;
        index += 1;
    }

    assert_eq!(
        10000,
        index,
        "There should be 10000 linear buckets of size 10000 usec between 0 and 100 sec."
    );
    assert_eq!(
        20000,
        total_added_counts,
        "Total added counts should be 20000"
    );

    index = 0;
    total_added_counts = 0;

    for value in histogram.linear_bucket_values(1000) {
        let count_added_in_this_bucket = value.count_added_in_this_iteration_step;
        if index == 1 {
            assert_eq!(
                10000,
                count_added_in_this_bucket,
                "Linear 1 sec bucket # 0 [{}..{}] added a count of 10000",
                value.value_iterated_from,
                value.value_iterated_to
            );
        }
        total_added_counts += value.count_added_in_this_iteration_step;
        index += 1;
    }

    assert_eq!(
        100007,
        index,
        "There should be 100007 linear buckets of size 1000 usec between 0 and 100 sec."
    );
    assert_eq!(
        20000,
        total_added_counts,
        "Total added counts should be 20000"
    );
}


#[test]
fn logarithmic_bucket_values() {
    let histogram = stat_histo();
    let raw_histogram = raw_stat_histo();

    let mut index = 0;

    for value in raw_histogram.logarithmic_bucket_values(10000, 2.0) {
        let count_added_in_this_bucket = value.count_added_in_this_iteration_step;
        if index == 0 {
            assert_eq!(
                10000,
                count_added_in_this_bucket,
                "Raw Logarithmic 10 msec bucket # 0 added a count of 10000"
            );
        } else if index == 14 {
            assert_eq!(
                1,
                count_added_in_this_bucket,
                "Raw Logarithmic 10 msec bucket # 14 added a count of 1"
            );
        } else {
            assert_eq!(
                0,
                count_added_in_this_bucket,
                "Raw Logarithmic 100 msec bucket # {} added a count of 0",
                index
            );
        }
        index += 1;
    }
    assert_eq!(14, index - 1);

    index = 0;
    let mut total_added_counts = 0;

    for value in histogram.logarithmic_bucket_values(10000, 2.0) {
        let count_added_in_this_bucket = value.count_added_in_this_iteration_step;
        if index == 0 {
            assert_eq!(
                10000,
                count_added_in_this_bucket,
                "Logarithmic 10 msec bucket # 0 [{}..{}] added a count of 10000",
                value.value_iterated_from,
                value.value_iterated_to
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
    assert_eq!(
        20000,
        total_added_counts,
        "Total added counts should be 20000"
    );
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
                10000,
                count_added_in_this_bucket,
                "Raw recorded value bucket # 0 added a count of 10000"
            );
        } else {
            assert_eq!(
                1,
                count_added_in_this_bucket,
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
                10000,
                count_added_in_this_bucket,
                "Recorded bucket # 0 [{}..{}] added a count of 10000",
                value.value_iterated_from,
                value.value_iterated_to
            );
        }
        assert!(
            value.count_at_value_iterated_to != 0,
            "The count in recorded bucket #{} is not 0",
            index
        );
        assert_eq!(
            value.count_at_value_iterated_to,
            value.count_added_in_this_iteration_step,
            "The count in recorded bucket # {} is exactly the amount added since the last iteration",
            index
        );
        total_added_counts += value.count_added_in_this_iteration_step;
        index += 1;
    }
    assert_eq!(
        20000,
        total_added_counts,
        "Total added counts should be 20000"
    );
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
            assert_eq!(
                10000,
                count_added_in_this_bucket,
                "Raw allValues bucket # 0 added a count of 10000"
            );
        } else if histogram.values_are_equivalent(value.value_iterated_to, 100000000) {
            assert_eq!(
                1,
                count_added_in_this_bucket,
                "Raw allValues value bucket # {} added a count of 1",
                index
            );
        } else {
            assert_eq!(
                0,
                count_added_in_this_bucket,
                "Raw allValues value bucket # {} added a count of 0",
                index
            );
        }
        latest_value_at_index = value.value_iterated_to;
        total_count_to_this_point += value.count_at_value_iterated_to;
        assert_eq!(
            total_count_to_this_point,
            value.total_count_to_this_value,
            "total Count should match"
        );
        total_value_to_this_point += value.count_at_value_iterated_to * latest_value_at_index;
        assert_eq!(
            total_value_to_this_point,
            value.total_value_to_this_value,
            "total Value should match"
        );
        index += 1;
    }
    assert_eq!(
        histogram.counts_array_length(),
        index,
        "index should be equal to countsArrayLength"
    );

    index = 0;
    let mut total_added_counts = 0;
    for value in histogram.all_values() {
        let count_added_in_this_bucket = value.count_added_in_this_iteration_step;
        if index == 1000 {
            assert_eq!(
                10000,
                count_added_in_this_bucket,
                "AllValues bucket # 0 [{}..{}] added a count of 10000",
                value.value_iterated_from,
                value.value_iterated_to
            );
        }
        assert_eq!(
            value.count_at_value_iterated_to,
            value.count_added_in_this_iteration_step,
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
    assert_eq!(
        histogram.counts_array_length(),
        index,
        "index should be equal to countsArrayLength"
    );
    assert_eq!(
        20000,
        total_added_counts,
        "Total added counts should be 20000"
    );
}
