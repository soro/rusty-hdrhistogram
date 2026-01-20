use crate::concurrent::resizable_histogram::ResizableHistogram;
use parking_lot::RwLock;
use rand::rngs::StdRng;
use std::sync::Arc;
use std::sync::Barrier;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use rand::{Rng, SeedableRng};

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

fn new_auto_resize_histogram() -> Arc<ResizableHistogram> {
    let histogram = Arc::new(ResizableHistogram::new(2).unwrap());
    histogram.set_auto_resize(true);
    histogram
}

#[test]
fn concurrent_auto_sized_recording() {
    const THREADS: usize = 8;
    const ITERATIONS: usize = 200;
    let histogram = new_auto_resize_histogram();
    let shared = Arc::new(RwLock::new(histogram));
    let ready_barrier = Arc::new(Barrier::new(THREADS + 1));
    let go_barrier = Arc::new(Barrier::new(THREADS + 1));
    let counts = Arc::new((0..THREADS).map(|_| AtomicU64::new(0)).collect::<Vec<_>>());
    let mut handles = Vec::with_capacity(THREADS);

    for tid in 0..THREADS {
        let shared = Arc::clone(&shared);
        let ready_barrier = Arc::clone(&ready_barrier);
        let go_barrier = Arc::clone(&go_barrier);
        let counts = Arc::clone(&counts);
        handles.push(thread::spawn(move || {
            let mut rng = StdRng::seed_from_u64(0xD1CEB00Fu64 ^ tid as u64);
            for _ in 0..ITERATIONS {
                ready_barrier.wait();
                go_barrier.wait();
                let value = rng.gen_range(1_u64..(1_u64 << 40));
                let histogram = shared.read().clone();
                succ!(histogram.resize(value));
                succ!(histogram.record_value(value));
                counts[tid].fetch_add(1, Ordering::Relaxed);
            }
        }));
    }

    for _ in 0..ITERATIONS {
        ready_barrier.wait();
        let sum = counts.iter().map(|c| c.load(Ordering::Relaxed)).sum::<u64>();
        let histogram = shared.read().clone();
        assert_eq!(sum, histogram.get_total_count());
        for counter in counts.iter() {
            counter.store(0, Ordering::Relaxed);
        }
        *shared.write() = new_auto_resize_histogram();
        go_barrier.wait();
    }

    for handle in handles {
        let _ = handle.join();
    }

    let sum = counts.iter().map(|c| c.load(Ordering::Relaxed)).sum::<u64>();
    let histogram = shared.read().clone();
    assert_eq!(sum, histogram.get_total_count());
}
