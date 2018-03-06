use concurrent::recordable_histogram::RecordableHistogram;
use concurrent::recorder::{self, Recorder};
use core::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;
use tests::rand::{self, Rng};

const HIGHEST_TRACKABLE: u64 = 3600 * 1000 * 1000;

#[test]
fn resizing_recorder() {
    let recorder = Arc::new(recorder::resizable_with_low_high_sigvdig(1, HIGHEST_TRACKABLE, 2).unwrap());
    run_recorder_test(recorder);
}

#[test]
fn static_recorder() {
    let recorder = Arc::new(recorder::static_with_low_high_sigvdig(1, HIGHEST_TRACKABLE, 2).unwrap());
    run_recorder_test(recorder);
}


fn run_recorder_test<T: 'static + RecordableHistogram>(recorder: Arc<Recorder<T>>) {
    const THREAD_COUNT: usize = 10;
    const NUM_VALS: usize = 1000000;
    let mut total_value = 0_u64;
    let ready_var = Arc::new(AtomicBool::new(false));
    let mut values = Vec::<Arc<Vec<u32>>>::new();
    let mut handles = Vec::<thread::JoinHandle<()>>::new();
    let mut rng = rand::weak_rng();

    for _ in 0..THREAD_COUNT {
        let mut vs = rng.gen_iter::<u32>()
            .take(NUM_VALS)
            .map(|v| {
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
