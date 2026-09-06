//! Small production-path regressions, also suitable for Miri. Recorder
//! timestamps require -Zmiri-disable-isolation. With crossbeam-epoch 0.9.18,
//! resize/retirement tests require Tree Borrows and -Zmiri-ignore-leaks:
//! Stacked Borrows rejects the dependency's intrusive-list access, while its
//! global collector can retain deferred allocations at process exit. The
//! backing-array unit tests need neither relaxation nor disabled leak checks.

use hdrhistogram::concurrent::{
    ConcurrentDoubleHistogram, FixedConcurrentHistogram, FixedRecorder, ResizableConcurrentHistogram, ResizableRecorder,
    SingleWriterRecorder,
};
use hdrhistogram::Histogram;

#[test]
fn fixed_inline_storage_records_and_drops() {
    drop(FixedConcurrentHistogram::builder().significant_digits(0).build().unwrap());
    let histogram = FixedConcurrentHistogram::builder()
        .significant_digits(0)
        .highest_trackable_value(1_024)
        .build()
        .unwrap();
    histogram.record_value(0).unwrap();
    histogram.record_value_with_count(1_024, 3).unwrap();
    assert_eq!(histogram.get_total_count(), 4);
    let index = histogram.settings().counts_array_index(1_024);
    assert_eq!(histogram.get_count_at_index(index), Some(3));
}

#[test]
fn resizable_inline_storage_preserves_counts_through_retirement() {
    let histogram = ResizableConcurrentHistogram::builder().significant_digits(0).build().unwrap();
    for value in [1, 16, 256, 4_096] {
        histogram.record_value_with_count(value, 2).unwrap();
    }
    assert_eq!(histogram.get_total_count(), 8);
    let settings = histogram.settings();
    for value in [1, 16, 256, 4_096] {
        assert_eq!(histogram.get_count_at_index(settings.counts_array_index(value)), Some(2));
    }
    let view = histogram.read_view();
    let mut recorded = view.recorded_values();
    let mut total = 0;
    while let Some(value) = recorded.try_next().unwrap() {
        total += value.count_added_in_this_iteration_step;
    }
    assert_eq!(total, 8);
}

#[test]
fn double_inline_storage_survives_range_shifts_and_growth() {
    let histogram = ConcurrentDoubleHistogram::builder().significant_digits(0).build().unwrap();
    for value in [1.0, 1.0 / 1_024.0, 1_024.0] {
        histogram.record_value_with_count(value, 2).unwrap();
    }
    assert_eq!(histogram.get_total_count(), 6);
    for value in [1.0, 1.0 / 1_024.0, 1_024.0] {
        assert_eq!(histogram.get_count_at_value(value), 2);
    }
}

#[test]
fn inline_storage_is_cleared_and_reused_by_recorders() {
    let fixed = FixedRecorder::builder().significant_digits(0).build().unwrap();
    let resizable = ResizableRecorder::builder().significant_digits(0).build().unwrap();
    for _ in 0..3 {
        fixed.record_value(1).unwrap();
        resizable.record_value(4_096).unwrap();
        assert_eq!(fixed.begin_interval_sample().snapshot().get_total_count(), 1);
        assert_eq!(resizable.begin_interval_sample().snapshot().get_total_count(), 1);
    }
}

#[test]
fn owned_slice_storage_grows_resets_and_is_sampled() {
    let mut histogram = Histogram::builder().significant_digits(0).build().unwrap();
    for value in [1, 16, 256, 4_096] {
        histogram.record_value(value).unwrap();
    }
    assert_eq!(histogram.get_total_count(), 4);
    for value in [1, 16, 256, 4_096] {
        assert_eq!(histogram.get_count_at_value(value), Some(1));
    }
    histogram.reset();
    assert_eq!(histogram.get_total_count(), 0);
    histogram.record_value(16).unwrap();
    assert_eq!(histogram.get_count_at_value(16), Some(1));

    let (mut writer, mut sampler) = SingleWriterRecorder::builder().significant_digits(0).build().unwrap();
    for value in [16, 256, 4_096] {
        writer.record_value(value).unwrap();
        let sample = sampler.begin_interval_sample();
        assert_eq!(sample.snapshot().get_total_count(), 1);
        assert_eq!(sample.snapshot().get_count_at_value(value), Some(1));
    }
}

#[test]
fn owned_slice_samples_remain_stable_while_the_writer_grows() {
    let (mut writer, mut sampler) = SingleWriterRecorder::builder().significant_digits(0).build().unwrap();
    std::thread::scope(|scope| {
        let recording = scope.spawn(move || {
            for _ in 0..8 {
                for value in [1, 16, 256] {
                    writer.record_value(value).unwrap();
                    std::thread::yield_now();
                }
            }
        });
        let mut sampled_total = 0;
        for _ in 0..8 {
            let sample = sampler.begin_interval_sample();
            let count = sample.snapshot().get_total_count();
            std::thread::yield_now();
            assert_eq!(sample.snapshot().get_total_count(), count);
            sampled_total += count;
        }
        recording.join().unwrap();
        sampled_total += sampler.begin_interval_sample().snapshot().get_total_count();
        assert_eq!(sampled_total, 24);
    });
}
