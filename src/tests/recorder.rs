use crate::concurrent::recordable_histogram::RecordableHistogram;
use crate::concurrent::recorder;
use crate::concurrent::{ResizableConcurrentHistogram, ResizableRecorder, StaticRecorder};
use crate::core::constants::ORIGINAL_MIN;
use crate::core::*;
use rand::Rng;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const HIGHEST_TRACKABLE: u64 = 3600 * 1000 * 1000;

#[test]
fn resizing_recorder() {
    let recorder = Arc::new(recorder::resizable_with_low_high_sigvdig(1, HIGHEST_TRACKABLE, 2).unwrap());
    run_resizable_recorder_test(recorder);
}

#[test]
fn static_recorder() {
    let recorder = Arc::new(recorder::static_with_low_high_sigvdig(1, HIGHEST_TRACKABLE, 2).unwrap());
    run_static_recorder_test(recorder);
}

#[test]
fn double_recorder_records_and_resamples() {
    let recorder = recorder::double_with_highest_to_lowest_value_ratio(1024, 2).unwrap();

    succ!(recorder.record_value(1.5));
    succ!(recorder.record_value_with_count(12.0, 2));

    let mut sample = recorder.locking_sample();
    assert_eq!(3, sample.histogram().get_total_count());
    assert_eq!(1, sample.histogram().get_count_at_value(1.5));
    assert_eq!(2, sample.histogram().get_count_at_value(12.0));
    assert_eq!(
        3,
        sample
            .recorded_values()
            .map(|value| value.count_added_in_this_iteration_step)
            .sum::<u64>()
    );

    succ!(recorder.record_value(24.0));
    sample = sample.resample();
    assert_eq!(1, sample.histogram().get_total_count());
    assert_eq!(1, sample.histogram().get_count_at_value(24.0));
}

#[test]
fn double_recorder_preserves_counts_across_conversion_ratio_changes() {
    let recorder = recorder::double_with_highest_to_lowest_value_ratio(1024, 2).unwrap();

    succ!(recorder.record_value(1.0));
    let mut sample = recorder.locking_sample();
    assert_eq!(1, sample.histogram().get_count_at_value(1.0));

    succ!(recorder.record_value(1_000_000.0));
    sample = sample.resample();
    assert_eq!(1, sample.histogram().get_total_count());
    assert_eq!(1, sample.histogram().get_count_at_value(1_000_000.0));
    assert!(sample.histogram().get_current_highest_trackable_value() > 1_000_000.0);
}

#[test]
fn double_recorder_snapshot_is_stable_after_resample() {
    let recorder = recorder::double(2).unwrap();

    succ!(recorder.record_value_with_expected_interval(100.0, 25.0));
    let mut sample = recorder.locking_sample();
    assert_eq!(4, sample.histogram().get_total_count());

    succ!(recorder.record_value(200.0));
    let first_total = sample.histogram().get_total_count();
    sample = sample.resample();
    assert_eq!(4, first_total);
    assert_eq!(1, sample.histogram().get_total_count());
    assert_eq!(1, sample.histogram().get_count_at_value(200.0));
}

#[test]
fn single_writer_recorder_records_and_resamples() {
    let recorder = recorder::single_writer_with_low_high_sigvdig(1, HIGHEST_TRACKABLE, 2).unwrap();

    succ!(recorder.record_value(100));
    succ!(recorder.record_value_with_count(1_000, 2));
    succ!(recorder.record_value_with_expected_interval(400, 100));

    let mut sample = recorder.locking_sample();
    assert_eq!(7, sample.snapshot().get_total_count());
    assert_eq!(Some(2), sample.histogram().get_count_at_value(100));
    assert_eq!(Some(2), sample.histogram().get_count_at_value(1_000));
    assert_eq!(Some(1), sample.histogram().get_count_at_value(400));
    assert_eq!(Some(1), sample.histogram().get_count_at_value(300));
    assert_eq!(Some(1), sample.histogram().get_count_at_value(200));
    assert!(sample.histogram().meta_data.end_timestamp.is_some());

    succ!(recorder.record_value(500));
    let first_total = sample.histogram().get_total_count();
    sample = sample.resample();
    assert_eq!(7, first_total);
    assert_eq!(1, sample.histogram().get_total_count());
    assert_eq!(Some(1), sample.histogram().get_count_at_value(500));
}

#[test]
fn single_writer_recorder_constructor_auto_resizes() {
    let recorder = recorder::single_writer(2).unwrap();

    succ!(recorder.record_value(HIGHEST_TRACKABLE));
    let sample = recorder.locking_sample();

    assert_eq!(1, sample.histogram().get_total_count());
    assert!(sample.histogram().get_highest_trackable_value() >= HIGHEST_TRACKABLE);
}

#[test]
fn single_writer_recorder_can_sample_while_single_writer_runs() {
    const ITERATIONS: usize = 20_000;
    let recorder = Arc::new(recorder::single_writer(2).unwrap());
    let ready = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));

    let writer = {
        let recorder = Arc::clone(&recorder);
        let ready = Arc::clone(&ready);
        let done = Arc::clone(&done);
        thread::spawn(move || {
            while !ready.load(Ordering::Acquire) {
                thread::yield_now();
            }
            for value in 1..=ITERATIONS {
                succ!(recorder.record_value(value as u64));
            }
            done.store(true, Ordering::Release);
        })
    };

    ready.store(true, Ordering::Release);
    let mut sample = recorder.locking_sample();
    let mut sampled_count = 0_u64;
    while !done.load(Ordering::Acquire) {
        sampled_count += sample.histogram().get_total_count();
        sample = sample.resample();
        thread::yield_now();
    }
    sampled_count += sample.histogram().get_total_count();
    writer.join().unwrap();
    sample = sample.resample();
    sampled_count += sample.histogram().get_total_count();

    assert_eq!(ITERATIONS as u64, sampled_count);
}

#[test]
fn single_writer_double_recorder_records_and_resamples() {
    let recorder = recorder::single_writer_double_with_highest_to_lowest_value_ratio(1024, 2).unwrap();

    succ!(recorder.record_value(1.5));
    succ!(recorder.record_value_with_count(12.0, 2));
    succ!(recorder.record_value_with_expected_interval(100.0, 25.0));

    let mut sample = recorder.locking_sample();
    assert_eq!(7, sample.snapshot().get_total_count());
    assert_eq!(1, sample.histogram().get_count_at_value(1.5));
    assert_eq!(2, sample.histogram().get_count_at_value(12.0));
    assert_eq!(1, sample.histogram().get_count_at_value(100.0));
    assert_eq!(1, sample.histogram().get_count_at_value(75.0));
    assert_eq!(1, sample.histogram().get_count_at_value(50.0));
    assert_eq!(1, sample.histogram().get_count_at_value(25.0));

    succ!(recorder.record_value(24.0));
    sample = sample.resample();
    assert_eq!(1, sample.histogram().get_total_count());
    assert_eq!(1, sample.histogram().get_count_at_value(24.0));
}

#[test]
fn single_writer_double_recorder_can_sample_while_single_writer_runs() {
    const ITERATIONS: usize = 20_000;
    let recorder = Arc::new(recorder::single_writer_double(2).unwrap());
    let ready = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));

    let writer = {
        let recorder = Arc::clone(&recorder);
        let ready = Arc::clone(&ready);
        let done = Arc::clone(&done);
        thread::spawn(move || {
            while !ready.load(Ordering::Acquire) {
                thread::yield_now();
            }
            for value in 1..=ITERATIONS {
                succ!(recorder.record_value(value as f64));
            }
            done.store(true, Ordering::Release);
        })
    };

    ready.store(true, Ordering::Release);
    let mut sample = recorder.locking_sample();
    let mut sampled_count = 0_u64;
    while !done.load(Ordering::Acquire) {
        sampled_count += sample.histogram().get_total_count();
        sample = sample.resample();
        thread::yield_now();
    }
    sampled_count += sample.histogram().get_total_count();
    writer.join().unwrap();
    sample = sample.resample();
    sampled_count += sample.histogram().get_total_count();

    assert_eq!(ITERATIONS as u64, sampled_count);
}

#[test]
fn single_writer_double_recorder_preserves_shifted_range_across_samples() {
    let recorder = recorder::single_writer_double_with_highest_to_lowest_value_ratio(1024, 2).unwrap();

    succ!(recorder.record_value(1_000_000.0));
    let mut sample = recorder.locking_sample();
    assert_eq!(1, sample.histogram().get_count_at_value(1_000_000.0));

    succ!(recorder.record_value(1_000_000.0));
    sample = sample.resample();
    assert_eq!(1, sample.histogram().get_total_count());
    assert_eq!(1, sample.histogram().get_count_at_value(1_000_000.0));
}

#[test]
fn saturating_single_writer_double_recorder_clamps_out_of_range_values() {
    let recorder = recorder::saturating_single_writer_double_with_highest_to_lowest_value_ratio(1024, 2).unwrap();

    succ!(recorder.record_value(f64::MAX));
    let sample = recorder.locking_sample();

    assert_eq!(1, sample.histogram().get_total_count());
    assert!(sample.histogram().get_max_value() < f64::MAX);
}

#[test]
fn recorder_resample_resets_tracking() {
    let recorder = recorder::resizable_with_low_high_sigvdig(1, HIGHEST_TRACKABLE, 2).unwrap();

    succ!(recorder.record_value(1));
    succ!(recorder.record_value(1000));

    let mut sample = recorder.locking_sample();
    {
        let snapshot = sample.snapshot();
        assert_eq!(snapshot.get_total_count(), 2);
        assert_eq!(snapshot.get_min_non_zero_value(), 1);
        let expected_max = snapshot.settings().highest_equivalent_value(1000);
        assert_eq!(snapshot.get_max_value(), expected_max);
    }

    succ!(recorder.record_value(500));
    sample = sample.resample();

    {
        let snapshot = sample.histogram();
        assert_eq!(snapshot.get_total_count(), 1);
        assert_eq!(snapshot.get_min_non_zero_value(), 500);
        let expected_max = snapshot.settings().highest_equivalent_value(500);
        assert_eq!(snapshot.get_max_value(), expected_max);
    }
}

#[test]
fn recorder_resample_does_not_lose_concurrent_counts() {
    const ITERATIONS: usize = 50_000;
    let recorder = Arc::new(recorder::resizable_with_low_high_sigvdig(1, HIGHEST_TRACKABLE, 2).unwrap());
    let ready = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));

    let writer = {
        let recorder = Arc::clone(&recorder);
        let ready = Arc::clone(&ready);
        let done = Arc::clone(&done);
        thread::spawn(move || {
            while !ready.load(Ordering::Acquire) {
                thread::yield_now();
            }
            for value in 1..=ITERATIONS {
                succ!(recorder.record_value(value as u64));
            }
            done.store(true, Ordering::Release);
        })
    };

    ready.store(true, Ordering::Release);
    let mut sample = recorder.locking_sample();
    let mut sampled_count = 0_u64;
    while !done.load(Ordering::Acquire) {
        sampled_count += sample.histogram().get_total_count();
        sample = sample.resample();
        thread::yield_now();
    }
    sampled_count += sample.histogram().get_total_count();
    writer.join().unwrap();
    sample = sample.resample();
    sampled_count += sample.histogram().get_total_count();

    assert_eq!(ITERATIONS as u64, sampled_count);
}

#[test]
fn clear_counts_resets_metadata() {
    let mut histogram = ResizableConcurrentHistogram::with_low_high_sigvdig(1, HIGHEST_TRACKABLE, 2).unwrap();

    histogram.meta_data_mut().set_tag_string("tag".to_string());
    histogram.meta_data_mut().set_start_now();
    histogram.meta_data_mut().set_end_now();
    succ!(histogram.record_value(123));

    unsafe { histogram.clear_counts() };

    assert_eq!(histogram.get_total_count(), 0);
    assert_eq!(histogram.get_min_non_zero_value(), ORIGINAL_MIN);
    assert_eq!(histogram.get_max_value(), 0);
    let meta_data = histogram.meta_data();
    assert!(meta_data.tag.is_none());
    assert!(meta_data.start_timestamp.is_none());
    assert!(meta_data.end_timestamp.is_none());
}

macro_rules! recorder_test {
    ($name:ident, $recorder:ty) => {
        fn $name(recorder: Arc<$recorder>) {
            const THREAD_COUNT: usize = 10;
            const NUM_VALS: usize = 1000000;
            let mut total_value = 0_u64;
            let ready_var = Arc::new(AtomicBool::new(false));
            let mut values = Vec::<Arc<Vec<u32>>>::new();
            let mut handles = Vec::<thread::JoinHandle<()>>::new();
            let mut rng = rand::thread_rng();

            for _ in 0..THREAD_COUNT {
                let vs = (&mut rng)
                    .sample_iter(rand::distributions::Standard)
                    .take(NUM_VALS)
                    .map(|v: u32| {
                        total_value += v as u64;
                        v
                    })
                    .collect::<Vec<u32>>();
                values.push(Arc::new(vs));
            }

            for i in 0..THREAD_COUNT {
                let ready_var = ready_var.clone();
                let recorder = recorder.clone();
                let vec = unsafe { values.get_unchecked(i).clone() };
                handles.push(thread::spawn(move || {
                    while !ready_var.load(Ordering::Acquire) {
                        thread::yield_now();
                    }
                    for v in vec.iter() {
                        succ!(recorder.record_value(*v as u64));
                    }
                }));
            }
            let keep_sampling = Arc::new(AtomicBool::new(true));
            let sampled_total_count = Arc::new(AtomicUsize::new(0));
            let sampled_total_value = Arc::new(AtomicUsize::new(0));
            {
                let ready_var = ready_var.clone();
                let keep_sampling = keep_sampling.clone();
                let sampled_total_count = sampled_total_count.clone();
                let sampled_total_value = sampled_total_value.clone();
                let recorder_c = recorder.clone();
                thread::spawn(move || {
                    while !ready_var.load(Ordering::Acquire) {
                        thread::yield_now();
                    }
                    let mut sample = recorder_c.locking_sample();
                    loop {
                        let total_sample_value = sample
                            .histogram()
                            .recorded_values()
                            .last()
                            .map(|v| v.total_value_to_this_value)
                            .unwrap_or(0);
                        sampled_total_count.fetch_add(sample.histogram().get_total_count() as usize, Ordering::Relaxed);
                        sampled_total_value.fetch_add(total_sample_value as usize, Ordering::Relaxed);
                        let flag = keep_sampling.load(Ordering::Acquire);
                        if !flag {
                            break;
                        } else {
                            thread::sleep(Duration::from_millis(1));
                            sample = sample.resample();
                        }
                    }
                });
            }

            ready_var.store(true, Ordering::Release);

            for handle in handles {
                let _ = handle.join();
            }

            keep_sampling.store(false, Ordering::SeqCst);

            let sample = recorder.locking_sample();
            let total_count = sample.histogram().get_total_count() as usize + sampled_total_count.load(Ordering::Relaxed);
            assert_eq!(total_count, (THREAD_COUNT * NUM_VALS) as usize);

            let total = values
                .iter()
                .fold(0_u64, |acc, vec| acc + vec.iter().fold(0, |acc, v| acc + *v as u64));

            let observed_value = sample
                .histogram()
                .recorded_values()
                .last()
                .map(|v| v.total_value_to_this_value as usize)
                .unwrap_or(0)
                + sampled_total_value.load(Ordering::Relaxed);

            assert_approx_eq!(total, observed_value, total as f64 * 0.005);
        }
    };
}

recorder_test!(run_resizable_recorder_test, ResizableRecorder);
recorder_test!(run_static_recorder_test, StaticRecorder);
