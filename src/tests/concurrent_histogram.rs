use crate::concurrent::{ConcurrentDoubleHistogram, ResizableConcurrentHistogram, SaturatingConcurrentDoubleHistogram, StaticHistogram};
use crate::core::RecordError;
use crate::iteration::IterationError;
use parking_lot::RwLock;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Barrier;
use std::thread;

#[test]
fn concurrent_record_value_overflow_throws() {
    let highest = 3600_u64 * 1000 * 1000;
    let resizable = ResizableConcurrentHistogram::with_high_sigvdig(highest, 2).unwrap();
    resizable.set_auto_resize(false);
    assert!(matches!(
        resizable.record_value(highest * 3),
        Err(RecordError::ValueOutOfRangeResizeDisabled)
    ));

    let static_histogram = StaticHistogram::with_low_high_sigvdig(1, highest, 2).unwrap();
    assert!(matches!(
        static_histogram.record_value(highest * 3),
        Err(RecordError::ValueOutOfRangeResizeDisabled)
    ));
}

#[test]
fn resizable_settings_snapshot_is_stable_after_resize() {
    let histogram = ResizableConcurrentHistogram::new(2).unwrap();
    histogram.set_auto_resize(true);

    let before = histogram.settings();
    succ!(histogram.record_value(1_000_000));
    let after = histogram.settings();

    assert_eq!(2, before.highest_trackable_value);
    assert!(after.highest_trackable_value >= 1_000_000);
    assert!(after.counts_array_length > before.counts_array_length);
}

#[test]
fn converted_double_recording_uses_shifted_active_ratio() {
    let histogram = ResizableConcurrentHistogram::with_low_high_sigvdig(1, 1024, 2).unwrap();
    histogram.set_integer_to_double_value_conversion_ratio(0.5);

    succ!(histogram.record_converted_double_value_with_count(2.0, 1));
    let index_before_shift = histogram.settings().counts_array_index(4);
    assert_eq!(Some(1), histogram.get_count_at_index(index_before_shift));

    succ!(histogram.shift_values_left_with_conversion_ratio(1, 0.25));

    succ!(histogram.record_converted_double_value_with_count(2.0, 1));
    let index_after_shift = histogram.settings().counts_array_index(8);
    assert_eq!(Some(2), histogram.get_count_at_index(index_after_shift));
    assert_eq!(Some(0), histogram.get_count_at_index(index_before_shift));
}

#[test]
fn zero_only_shift_updates_active_ratio() {
    let histogram = ResizableConcurrentHistogram::with_low_high_sigvdig(1, 1024, 2).unwrap();
    histogram.set_integer_to_double_value_conversion_ratio(0.5);
    succ!(histogram.record_value(0));

    succ!(histogram.shift_values_left_with_conversion_ratio(1, 0.25));

    assert_eq!(0.25, histogram.integer_to_double_value_conversion_ratio());
    assert_ne!(0, histogram.normalizing_index_offset());
    succ!(histogram.record_converted_double_value_with_count(2.0, 1));
    let shifted_index = histogram.settings().counts_array_index(8);
    assert_eq!(Some(1), histogram.get_count_at_index(shifted_index));
}

#[test]
fn concurrent_double_empty_range_shift_publishes_conversion_ratio() {
    let histogram = ConcurrentDoubleHistogram::with_highest_to_lowest_value_ratio(1024, 2).unwrap();

    succ!(histogram.record_value(1.0));

    assert_eq!(1, histogram.get_total_count());
    assert_eq!(1, histogram.get_count_at_value(1.0));
    assert!(histogram.get_current_lowest_trackable_non_zero_value() <= 1.0);
    assert!(histogram.get_current_highest_trackable_value() > 1.0);
}

#[test]
fn concurrent_double_zero_only_range_shift_publishes_conversion_ratio() {
    let histogram = ConcurrentDoubleHistogram::with_highest_to_lowest_value_ratio(1024, 2).unwrap();

    succ!(histogram.record_value(0.0));
    succ!(histogram.record_value(1.0));

    assert_eq!(2, histogram.get_total_count());
    assert_eq!(1, histogram.get_count_at_value(0.0));
    assert_eq!(1, histogram.get_count_at_value(1.0));
    assert!(histogram.get_current_lowest_trackable_non_zero_value() <= 1.0);
}

#[test]
fn saturating_concurrent_double_clamps_after_failed_range_shift() {
    let histogram = SaturatingConcurrentDoubleHistogram::with_highest_to_lowest_value_ratio(1024, 2).unwrap();

    succ!(histogram.record_value(f64::MAX));
    succ!(histogram.record_value(12_340.0));

    assert_eq!(2, histogram.get_total_count());
}

#[test]
fn concurrent_double_range_shift_publication_under_writers() {
    const THREADS: usize = 4;
    const ITERATIONS: usize = 2_000;
    let histogram = Arc::new(ConcurrentDoubleHistogram::new(2).unwrap());
    let ready = Arc::new(Barrier::new(THREADS + 1));
    let mut handles = Vec::with_capacity(THREADS);

    for tid in 0..THREADS {
        let histogram = Arc::clone(&histogram);
        let ready = Arc::clone(&ready);
        handles.push(thread::spawn(move || {
            ready.wait();
            for i in 0..ITERATIONS {
                let value = match (tid + i) % 4 {
                    0 => 0.0,
                    1 => 1.0 / 1024.0,
                    2 => 1.0,
                    _ => 1_000_000.0,
                };
                succ!(histogram.record_value(value));
            }
        }));
    }

    ready.wait();
    for handle in handles {
        handle.join().unwrap();
    }

    assert_eq!((THREADS * ITERATIONS) as u64, histogram.get_total_count());
    assert!(histogram.get_count_at_value(0.0) > 0);
    assert!(histogram.get_count_at_value(1.0 / 1024.0) > 0);
    assert!(histogram.get_count_at_value(1.0) > 0);
    assert!(histogram.get_count_at_value(1_000_000.0) > 0);
}

#[test]
fn resizable_read_view_iteration_reports_concurrent_modification() {
    let histogram = ResizableConcurrentHistogram::new(2).unwrap();
    succ!(histogram.record_value(1));

    let view = histogram.read_view();
    let mut iterator = view.recorded_values();
    succ!(histogram.record_value(1));

    assert_eq!(Err(IterationError::ConcurrentModification), iterator.try_next());
}

#[test]
fn concurrent_double_read_view_iteration_reports_concurrent_modification() {
    let histogram = ConcurrentDoubleHistogram::new(2).unwrap();
    succ!(histogram.record_value(1.0));

    let view = histogram.read_view();
    let mut iterator = view.recorded_values();
    succ!(histogram.record_value(1.0));

    assert_eq!(Err(IterationError::ConcurrentModification), iterator.try_next());
}

#[test]
fn concurrent_double_try_get_mean_reports_concurrent_modification() {
    let histogram = ConcurrentDoubleHistogram::new(2).unwrap();
    succ!(histogram.record_value(1.0));

    let view = histogram.read_view();
    succ!(histogram.record_value(1.0));

    assert_eq!(Err(IterationError::ConcurrentModification), view.try_get_mean());
}

#[test]
fn concurrent_clear_counts_resets_normalizing_offset() {
    let histogram = ResizableConcurrentHistogram::with_low_high_sigvdig(1, 1024, 2).unwrap();

    succ!(histogram.record_value(4));
    succ!(histogram.shift_values_left(1));
    assert_ne!(0, histogram.normalizing_index_offset());

    unsafe { histogram.clear_counts() };
    assert_eq!(0, histogram.normalizing_index_offset());

    succ!(histogram.record_value(4));
    let index = histogram.settings().counts_array_index(4);
    assert_eq!(Some(1), histogram.get_count_at_index(index));
}

#[test]
fn concurrent_resize_preserves_counts_after_right_shift() {
    let histogram = ResizableConcurrentHistogram::with_high_sigvdig(1_024, 2).unwrap();
    succ!(histogram.record_value(512));
    succ!(histogram.shift_values_right(1));

    let shifted_index = histogram.settings().counts_array_index(256);
    assert_eq!(Some(1), histogram.get_count_at_index(shifted_index));

    succ!(histogram.resize(1_000_000));

    assert_eq!(1, histogram.get_total_count());
    let shifted_index = histogram.settings().counts_array_index(256);
    assert_eq!(Some(1), histogram.get_count_at_index(shifted_index));
}

#[test]
fn concurrent_record_values() {
    const THREAD_COUNT: usize = 10;
    const NUM_VALS: usize = 1000000;
    let mut total_value = 0_u64;
    let ready_var = Arc::new(AtomicBool::new(false));
    let mut values = Vec::<Arc<Vec<u32>>>::new();
    let mut handles = Vec::<thread::JoinHandle<()>>::new();
    let histogram = Arc::new(ResizableConcurrentHistogram::new(2).unwrap());
    let mut rng = rand::thread_rng();
    histogram.set_auto_resize(true);

    // TODO: pick from larger range here
    for _ in 0..THREAD_COUNT {
        let vs = (&mut rng)
            .sample_iter(rand::distributions::Standard)
            .take(NUM_VALS)
            .inspect(|v: &u32| {
                total_value += *v as u64;
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

    assert_eq!(histogram.get_total_count(), (THREAD_COUNT * NUM_VALS) as u64);

    let observed_value = unsafe { histogram.unsafe_as_snapshot() }
        .recorded_values()
        .last()
        .unwrap()
        .total_value_to_this_value;
    let total = values
        .iter()
        .fold(0_u64, |acc, vec| acc + vec.iter().fold(0, |acc, v| acc + *v as u64));
    assert_approx_eq!(total, observed_value, total as f64 * 0.005);
}

fn new_auto_resize_histogram() -> Arc<ResizableConcurrentHistogram> {
    let histogram = Arc::new(ResizableConcurrentHistogram::new(2).unwrap());
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
