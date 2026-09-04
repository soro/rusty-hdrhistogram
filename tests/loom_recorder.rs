#[path = "support/loom_model.rs"]
mod loom_model;

use loom::sync::atomic::Ordering;
use loom::thread;
use loom_model::{arc, atomic_usize, model_single_writer_handles, run_model, spawn, ModelRecorder};

#[test]
fn resample_never_exposes_a_histogram_still_mutated_by_old_phase_writers() {
    run_model(|| {
        let recorder = arc(ModelRecorder::new());
        let sampled_histogram = arc(atomic_usize(usize::MAX));
        let sampled_count_at_flip = arc(atomic_usize(usize::MAX));

        let writer = {
            let recorder = recorder.clone();
            spawn(move || {
                recorder.record_one();
            })
        };

        let sampler = {
            let recorder = recorder.clone();
            let sampled_histogram = sampled_histogram.clone();
            let sampled_count_at_flip = sampled_count_at_flip.clone();
            spawn(move || {
                let sampled = recorder.resample_into(1);
                let count = recorder.count(sampled);
                sampled_histogram.store(sampled, Ordering::Release);
                sampled_count_at_flip.store(count, Ordering::Release);
                thread::yield_now();
                assert_eq!(count, recorder.count(sampled));
            })
        };

        writer.join().unwrap();
        sampler.join().unwrap();

        let sampled = sampled_histogram.load(Ordering::Acquire);
        let count_at_flip = sampled_count_at_flip.load(Ordering::Acquire);
        assert_eq!(count_at_flip, recorder.count(sampled));
        assert_eq!(1, recorder.total_count());
    });
}

#[test]
fn resample_publishes_cleared_inactive_histogram_to_new_writers() {
    run_model(|| {
        let recorder = arc(ModelRecorder::new());
        recorder.set_count(1, 7);

        let sampler = {
            let recorder = recorder.clone();
            spawn(move || {
                recorder.resample_into(1);
            })
        };

        let writer = {
            let recorder = recorder.clone();
            spawn(move || {
                recorder.wait_until_active(1);
                recorder.record_one();
            })
        };

        sampler.join().unwrap();
        writer.join().unwrap();

        assert_eq!(1, recorder.count(1));
    });
}

#[test]
fn single_writer_split_keeps_sample_stable_and_recycles_on_drop() {
    run_model(|| {
        let (mut writer_handle, mut sampler_handle) = model_single_writer_handles();

        let writer = spawn(move || {
            writer_handle.record_one();
        });

        let first_sample = sampler_handle.begin_interval_sample();
        let first_count = first_sample.count();
        thread::yield_now();
        writer.join().unwrap();
        assert_eq!(first_count, first_sample.count());
        drop(first_sample);

        let second_sample = sampler_handle.begin_interval_sample();
        let second_count = second_sample.count();
        assert_eq!(1, first_count + second_count);
        drop(second_sample);
    });
}
