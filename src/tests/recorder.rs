use crate::concurrent::recordable_histogram::RecordableHistogram;
use crate::concurrent::resizable_histogram::ResizableHistogram;
use crate::concurrent::recorder::{self, Recorder};
use crate::core::constants::ORIGINAL_MIN;
use crate::core::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;
use rand::Rng;

const HIGHEST_TRACKABLE: u64 = 3600 * 1000 * 1000;
const STATIC_COUNTS_LEN: usize = 3328;

#[test]
fn resizing_recorder() {
    let recorder = Arc::new(recorder::resizable_with_low_high_sigvdig(1, HIGHEST_TRACKABLE, 2).unwrap());
    run_recorder_test(recorder);
}

#[test]
fn static_recorder() {
    let recorder = Arc::new(recorder::static_with_low_high_sigvdig::<STATIC_COUNTS_LEN>(1, HIGHEST_TRACKABLE, 2).unwrap());
    run_recorder_test(recorder);
}

#[test]
fn recorder_resample_resets_tracking() {
    let recorder = recorder::resizable_with_low_high_sigvdig(1, HIGHEST_TRACKABLE, 2).unwrap();

    succ!(recorder.record_value(1));
    succ!(recorder.record_value(1000));

    let mut sample = recorder.locking_sample();
    {
        let snapshot = sample.histogram();
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
fn clear_counts_resets_metadata() {
    let mut histogram = ResizableHistogram::with_low_high_sigvdig(
        1,
        HIGHEST_TRACKABLE,
        2,
    )
    .unwrap();

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


fn run_recorder_test<T: 'static + RecordableHistogram>(recorder: Arc<Recorder<T>>) {
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
                sampled_total_count.fetch_add(
                    sample.histogram().get_total_count() as usize,
                    Ordering::Relaxed,
                );
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

    let total = values.iter().fold(0_u64, |acc, vec| {
        acc + vec.iter().fold(0, |acc, v| acc + *v as u64)
    });

    let observed_value = sample
        .histogram()
        .recorded_values()
        .last()
        .map(|v| v.total_value_to_this_value as usize)
        .unwrap_or(0) + sampled_total_value.load(Ordering::Relaxed);

    assert_approx_eq!(total, observed_value, total as f64 * 0.005);
}
