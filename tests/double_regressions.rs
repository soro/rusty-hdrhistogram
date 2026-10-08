//! Read-side and correction regressions. Miri runs involving recorder sampling
//! need -Zmiri-disable-isolation; resizes additionally need Tree Borrows and
//! -Zmiri-ignore-leaks for crossbeam-epoch's deferred reclamation.

use hdrhistogram::concurrent::{ConcurrentDoubleHistogram, SaturatingConcurrentDoubleHistogram};
use hdrhistogram::encoding::{decode_double_histogram_v2, encode_concurrent_double_read_view_v2, encode_concurrent_double_snapshot_v2};
use hdrhistogram::{
    DoubleHistogram, DoubleRecorder, RecordError, SaturatingDoubleHistogram, SaturatingDoubleRecorder,
    SaturatingSingleWriterDoubleRecorder, SingleWriterDoubleRecorder,
};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

fn completes_without_deadlock(test: impl FnOnce() + Send + 'static) {
    let (finished, completion) = mpsc::channel();
    let worker = thread::spawn(move || {
        test();
        finished.send(()).unwrap();
    });
    completion
        .recv_timeout(Duration::from_secs(if cfg!(miri) { 180 } else { 30 }))
        .expect("operation stalled or panicked");
    worker.join().unwrap();
}

#[test]
fn frozen_double_snapshot_supports_overlapping_reads_and_next_interval_writers() {
    completes_without_deadlock(|| {
        let histogram = ConcurrentDoubleHistogram::builder().significant_digits(0).build().unwrap();
        for value in [4.0, 1.0 / 1_024.0, 4_096.0, 0.0, 16.0] {
            histogram.record_value_with_count(value, 2).unwrap();
        }
        // Capture a reference before the recorder handoff, using the same
        // conversion ratio. Independent constructors can round their powi-based
        // initial ranges differently at bucket boundaries (notably in Miri).
        let reference = decode_double_histogram_v2(&encode_concurrent_double_read_view_v2(&histogram.read_view()).unwrap()).unwrap();
        let recorder = DoubleRecorder::from_histogram(histogram);
        let sample = recorder.begin_interval_sample();
        {
            let snapshot = sample.snapshot();
            let mut first = snapshot.recorded_values();
            let second = sample.recorded_values();
            let another_snapshot = sample.snapshot();
            let view = snapshot.read_view();
            let all = snapshot.all_values();
            let linear = snapshot.linear_bucket_values(1_024.0);
            let logarithmic = snapshot.logarithmic_bucket_values(1.0, 2.0).unwrap();
            let percentiles = snapshot.percentiles(2);

            assert_eq!(reference.get_max_value(), snapshot.get_max_value());
            assert_eq!(reference.get_min_value(), snapshot.get_min_value());
            assert_eq!(reference.get_mean(), snapshot.try_get_mean().unwrap());
            assert_eq!(reference.get_std_deviation(), snapshot.try_get_std_deviation().unwrap());
            for percentile in [0.0, 50.0, 100.0] {
                assert_eq!(
                    reference.get_value_at_percentile(percentile),
                    snapshot.get_value_at_percentile(percentile)
                );
            }
            let encoded = encode_concurrent_double_snapshot_v2(&snapshot).unwrap();
            let decoded = decode_double_histogram_v2(&encoded).unwrap();
            assert_eq!(10, decoded.get_total_count());
            assert!(std::ptr::eq(view.meta_data(), another_snapshot.meta_data()));

            thread::scope(|scope| {
                // The frozen view is shareable without a mutex or epoch guard.
                scope.spawn(|| {
                    for _ in 0..20 {
                        assert_eq!(10, view.get_total_count());
                        assert_eq!(2, view.get_count_at_value(1.0 / 1_024.0));
                    }
                });
                for value in 0..100 {
                    recorder.record_value(value as f64).unwrap();
                    assert_eq!(10, snapshot.get_total_count());
                }
            });

            assert!(first.try_next().unwrap().is_some());
            first.reset();
            assert_eq!(10, first.map(|value| value.count_added_in_this_iteration_step).sum::<u64>());
            assert_eq!(10, second.map(|value| value.count_added_in_this_iteration_step).sum::<u64>());
            assert_eq!(10, all.map(|value| value.count_added_in_this_iteration_step).sum::<u64>());
            assert_eq!(10, linear.map(|value| value.count_added_in_this_iteration_step).sum::<u64>());
            assert_eq!(10, logarithmic.map(|value| value.count_added_in_this_iteration_step).sum::<u64>());
            assert_eq!(10, percentiles.map(|value| value.count_added_in_this_iteration_step).sum::<u64>());
            assert!(snapshot.meta_data().start_timestamp.is_some());
            assert!(snapshot.meta_data().end_timestamp.is_some());
        }
        let next = sample.resample();
        assert_eq!(100, next.snapshot().get_total_count());
    });
}

#[test]
fn empty_frozen_double_sample_supports_overlapping_queries_and_encoding() {
    completes_without_deadlock(|| {
        let recorder = DoubleRecorder::new();
        let sample = recorder.begin_interval_sample();
        let mut recorded = sample.recorded_values();
        let snapshot = sample.snapshot();
        assert_eq!(0, snapshot.get_total_count());
        assert_eq!(0.0, snapshot.try_get_mean().unwrap());
        assert_eq!(0.0, snapshot.try_get_std_deviation().unwrap());
        let encoded = encode_concurrent_double_snapshot_v2(&snapshot).unwrap();
        assert_eq!(0, decode_double_histogram_v2(&encoded).unwrap().get_total_count());
        assert!(recorded.next().is_none());
    });
}

#[test]
fn rejected_tiny_double_values_leave_usable_range_state() {
    completes_without_deadlock(|| {
        macro_rules! check {
            ($histogram:expr) => {{
                #[allow(unused_mut)]
                let mut histogram = $histogram;
                assert_eq!(
                    Err(RecordError::ValueOutOfRangeResizeDisabled),
                    histogram.record_value(f64::from_bits(1))
                );
                assert_eq!(0, histogram.get_total_count());
                assert!(histogram.get_max_value().is_finite());
                histogram.record_value_with_count(0.0, 3).unwrap();
                assert_eq!(
                    Err(RecordError::ValueOutOfRangeResizeDisabled),
                    histogram.record_value_with_count(f64::from_bits(1), 5)
                );
                assert_eq!(3, histogram.get_total_count());
                assert_eq!(3, histogram.get_count_at_value(0.0));
                assert!(histogram.get_max_value().is_finite());
                histogram.record_value(1.0).unwrap();
                assert_eq!(4, histogram.get_total_count());
                assert_eq!(1, histogram.get_count_at_value(1.0));
                assert!(histogram.get_max_value().is_finite());
            }};
        }
        for digits in [0, 3] {
            // Larger shifts keep the boundary exercise inexpensive in Miri.
            check!(DoubleHistogram::builder()
                .significant_digits(digits)
                .highest_to_lowest_value_ratio(1_024)
                .build()
                .unwrap());
            check!(ConcurrentDoubleHistogram::builder()
                .significant_digits(digits)
                .highest_to_lowest_value_ratio(1_024)
                .build()
                .unwrap());
        }
    });
}

#[test]
fn double_range_growth_preserves_finite_bounds_near_float_limit() {
    completes_without_deadlock(|| {
        macro_rules! check {
            ($histogram:expr) => {{
                #[allow(unused_mut)]
                let mut histogram = $histogram;
                let high = f64::from_bits((1_020 + 1_023) << 52);
                let low = f64::from_bits((995 + 1_023) << 52);
                histogram.record_value(high).unwrap();
                // Keeping the high bound while growing downward must not
                // divide it by the shift multiplier and then multiply back:
                // that intermediate value overflows despite a valid result.
                histogram.record_value(low).unwrap();
                assert_eq!(2, histogram.get_total_count());
                assert_eq!(1, histogram.get_count_at_value(high));
                assert_eq!(1, histogram.get_count_at_value(low));
                assert!(histogram.get_max_value().is_finite());
            }};
        }
        check!(DoubleHistogram::builder()
            .significant_digits(0)
            .highest_to_lowest_value_ratio(1_024)
            .build()
            .unwrap());
        check!(ConcurrentDoubleHistogram::builder()
            .significant_digits(0)
            .highest_to_lowest_value_ratio(1_024)
            .build()
            .unwrap());
    });
}

#[test]
fn saturating_tiny_double_values_keep_queries_and_later_records_usable() {
    completes_without_deadlock(|| {
        macro_rules! check {
            ($histogram:expr) => {{
                #[allow(unused_mut)]
                let mut histogram = $histogram;
                histogram.record_value(f64::from_bits(1)).unwrap();
                histogram.record_value_with_count(f64::from_bits(1), 2).unwrap();
                assert_eq!(3, histogram.get_total_count());
                let clamped = histogram.get_min_value();
                assert!(clamped.is_finite() && clamped > f64::from_bits(1));
                assert!(histogram.get_max_value().is_finite());
                assert_eq!(3, histogram.get_count_at_value(clamped));
                histogram.record_value(clamped).unwrap();
                assert_eq!(4, histogram.get_total_count());
            }};
        }
        check!(SaturatingDoubleHistogram::builder()
            .significant_digits(0)
            .highest_to_lowest_value_ratio(1_024)
            .build()
            .unwrap());
        check!(SaturatingConcurrentDoubleHistogram::builder()
            .significant_digits(0)
            .highest_to_lowest_value_ratio(1_024)
            .build()
            .unwrap());
    });
}

#[test]
fn double_recorders_can_sample_after_a_tiny_value_error() {
    completes_without_deadlock(|| {
        let recorder = DoubleRecorder::builder()
            .significant_digits(0)
            .highest_to_lowest_value_ratio(1_024)
            .build()
            .unwrap();
        assert_eq!(
            Err(RecordError::ValueOutOfRangeResizeDisabled),
            recorder.record_value(f64::from_bits(1))
        );
        let sample = recorder.begin_interval_sample();
        assert_eq!(0, sample.snapshot().get_total_count());
        assert!(sample.snapshot().get_max_value().is_finite());
        recorder.record_value(1.0).unwrap();
        assert_eq!(1, sample.resample().snapshot().get_total_count());

        let (mut writer, mut sampler) = SingleWriterDoubleRecorder::builder()
            .significant_digits(0)
            .highest_to_lowest_value_ratio(1_024)
            .build()
            .unwrap();
        assert_eq!(
            Err(RecordError::ValueOutOfRangeResizeDisabled),
            writer.record_value(f64::from_bits(1))
        );
        let sample = sampler.begin_interval_sample();
        assert_eq!(0, sample.snapshot().get_total_count());
        assert!(sample.snapshot().get_max_value().is_finite());
        writer.record_value(1.0).unwrap();
        assert_eq!(1, sample.resample().snapshot().get_total_count());
    });
}

fn exercise_correction(mut record: impl FnMut(f64, f64) -> Result<(), RecordError>) -> u64 {
    // Immediate rounding stall: the real measurement remains recorded.
    assert_eq!(Err(RecordError::InvalidExpectedInterval), record(1.0, f64::EPSILON / 4.0));
    // The first subtraction progresses to 1.5; the next ties back to 1.5.
    assert_eq!(
        Err(RecordError::InvalidExpectedInterval),
        record(f64::from_bits(1.5_f64.to_bits() + 1), f64::EPSILON / 2.0)
    );
    for interval in [f64::NAN, f64::INFINITY] {
        assert_eq!(Err(RecordError::InvalidExpectedInterval), record(1.0, interval));
    }
    // Non-positive intervals still disable correction, as in Java.
    for interval in [0.0, -1.0, f64::NEG_INFINITY] {
        record(1.0, interval).unwrap();
    }
    record(8.0, 2.0).unwrap(); // Actual 8, followed by 6, 4 and 2.
    1 + 2 + 2 + 3 + 4
}

#[test]
fn direct_double_correction_detects_rounding_stalls_and_invalid_intervals() {
    completes_without_deadlock(|| {
        macro_rules! check {
            ($histogram:expr) => {{
                #[allow(unused_mut)]
                let mut histogram = $histogram;
                let expected = exercise_correction(|value, interval| histogram.record_value_with_expected_interval(value, interval));
                assert_eq!(expected, histogram.get_total_count());
                histogram.record_value(42.0).unwrap();
                assert_eq!(expected + 1, histogram.get_total_count());
                assert!(matches!(
                    histogram.copy_corrected_for_coordinated_omission(f64::EPSILON / 4.0),
                    Err(RecordError::InvalidExpectedInterval)
                ));
                assert_eq!(expected + 1, histogram.get_total_count());
            }};
        }
        // Small backing arrays keep these production-path checks cheap in Miri.
        check!(DoubleHistogram::builder().significant_digits(0).build().unwrap());
        check!(SaturatingDoubleHistogram::builder().significant_digits(0).build().unwrap());
        check!(ConcurrentDoubleHistogram::builder().significant_digits(0).build().unwrap());
        check!(SaturatingConcurrentDoubleHistogram::builder()
            .significant_digits(0)
            .build()
            .unwrap());
    });
}

#[test]
fn recorder_double_correction_releases_writer_phase_after_error() {
    completes_without_deadlock(|| {
        macro_rules! check {
            ($recorder:expr) => {{
                let recorder = $recorder;
                let expected =
                    exercise_correction(|value, interval| recorder.record_value_with_count_and_expected_interval(value, 3, interval));
                let sample = recorder.begin_interval_sample();
                assert_eq!(3 * expected, sample.snapshot().get_total_count());
                recorder.record_value(42.0).unwrap();
                assert_eq!(1, sample.resample().snapshot().get_total_count());
            }};
        }
        check!(DoubleRecorder::builder().significant_digits(0).build().unwrap());
        check!(SaturatingDoubleRecorder::builder().significant_digits(0).build().unwrap());

        macro_rules! check_single_writer {
            ($handles:expr) => {{
                let (mut writer, mut sampler) = $handles;
                let expected =
                    exercise_correction(|value, interval| writer.record_value_with_count_and_expected_interval(value, 3, interval));
                let sample = sampler.begin_interval_sample();
                assert_eq!(3 * expected, sample.snapshot().get_total_count());
                writer.record_value(42.0).unwrap();
                assert_eq!(1, sample.resample().snapshot().get_total_count());
            }};
        }
        check_single_writer!(SingleWriterDoubleRecorder::builder().significant_digits(0).build().unwrap());
        check_single_writer!(SaturatingSingleWriterDoubleRecorder::builder()
            .significant_digits(0)
            .build()
            .unwrap());
    });
}

#[cfg(feature = "encoding-compression")]
#[test]
fn frozen_double_snapshot_compresses_while_iterators_are_alive() {
    completes_without_deadlock(|| {
        use hdrhistogram::encoding::{decode_double_histogram_compressed, encode_concurrent_double_snapshot_compressed_with_level};
        let recorder = DoubleRecorder::new();
        recorder.record_value(42.0).unwrap();
        let sample = recorder.begin_interval_sample();
        let snapshot = sample.snapshot();
        let mut values = snapshot.recorded_values();
        let encoded = encode_concurrent_double_snapshot_compressed_with_level(&snapshot, 9).unwrap();
        assert_eq!(1, decode_double_histogram_compressed(&encoded).unwrap().get_count_at_value(42.0));
        assert_eq!(1, values.next().unwrap().count_added_in_this_iteration_step);
    });
}

#[cfg(feature = "encoding-base64")]
#[test]
fn frozen_double_snapshot_encodes_log_line_while_iterators_are_alive() {
    completes_without_deadlock(|| {
        use hdrhistogram::encoding::{
            decode_histogram_log_line, encode_concurrent_double_snapshot_log_line, DecodedHistogram, HistogramLogRecord,
        };
        let recorder = DoubleRecorder::new();
        recorder.record_value(42.0).unwrap();
        let sample = recorder.begin_interval_sample();
        let snapshot = sample.snapshot();
        let mut values = snapshot.recorded_values();
        let encoded = encode_concurrent_double_snapshot_log_line(&snapshot, 1.0, 2.0).unwrap();
        let Some(HistogramLogRecord::Interval(entry)) = decode_histogram_log_line(&encoded).unwrap() else {
            panic!("expected an interval log record");
        };
        let DecodedHistogram::Double(decoded) = entry.histogram else {
            panic!("expected a double histogram");
        };
        assert_eq!(1, decoded.get_count_at_value(42.0));
        assert_eq!(1, values.next().unwrap().count_added_in_this_iteration_step);
    });
}
