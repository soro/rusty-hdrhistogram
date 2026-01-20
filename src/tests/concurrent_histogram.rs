use crate::concurrent::resizable_histogram::ResizableHistogram;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use rand::Rng;

#[test]
fn concurrent_record_values() {
    const THREAD_COUNT: usize = 10;
    const NUM_VALS: usize = 1000000;
    let mut total_value = 0_u64;
    let ready_var = Arc::new(AtomicBool::new(false));
    let mut values = Vec::<Arc<Vec<u32>>>::new();
    let mut handles = Vec::<thread::JoinHandle<()>>::new();
    let histogram = Arc::new(ResizableHistogram::new(2).unwrap());
    let mut rng = rand::thread_rng();
    histogram.set_auto_resize(true);

    // TODO: pick from larger range here
    for _ in 0..THREAD_COUNT {
        let mut vs = rng
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
        let histogram = histogram.clone();
        let vec = unsafe { values.get_unchecked(i).clone() };
        handles.push(thread::spawn(move || {
            while !ready_var.load(Ordering::Acquire) {
                thread::yield_now();
            }
            for v in vec.iter() {
                succ!(histogram.record_value(*v as u64));
            }
        }));
    }

    ready_var.store(true, Ordering::Release);

    for handle in handles {
        let _ = handle.join();
    }

    assert_eq!(
        histogram.get_total_count(),
        (THREAD_COUNT * NUM_VALS) as u64
    );

    let observed_value = unsafe { histogram.unsafe_as_snapshot() }
        .recorded_values()
        .last()
        .unwrap()
        .total_value_to_this_value;
    let total = values.iter().fold(0_u64, |acc, vec| {
        acc + vec.iter().fold(0, |acc, v| acc + *v as u64)
    });
    assert_approx_eq!(total, observed_value, total as f64 * 0.005);
}
