#[path = "support/loom_model.rs"]
mod loom_model;

use loom::sync::atomic::Ordering;
use loom::thread;
use loom_model::{arc, atomic_bool, run_model, spawn, ModelResizableConcurrentHistogram};

#[test]
fn read_view_keeps_captured_backing_arrays_live_across_racing_resize() {
    run_model(|| {
        let histogram = arc(ModelResizableConcurrentHistogram::new());

        let reader = {
            let histogram = histogram.clone();
            spawn(move || {
                let view = histogram.read_view();
                view.assert_captured_pair_is_live_and_consistent();
                thread::yield_now();
                view.assert_captured_pair_is_live_and_consistent();
                let _ = view.total_count();
                thread::yield_now();
                view.assert_captured_pair_is_live_and_consistent();
            })
        };

        let resizer = {
            let histogram = histogram.clone();
            spawn(move || {
                histogram.resize_to_second_pair();
            })
        };

        reader.join().unwrap();
        resizer.join().unwrap();

        assert!(histogram.active_pair_is_second_pair());
        assert!(histogram.old_pair_is_retired());
    });
}

#[test]
fn resize_cannot_retire_arrays_while_an_existing_read_view_is_alive() {
    run_model(|| {
        let histogram = arc(ModelResizableConcurrentHistogram::new());
        let view_ready = arc(atomic_bool(false));
        let release_view = arc(atomic_bool(false));

        let reader = {
            let histogram = histogram.clone();
            let view_ready = view_ready.clone();
            let release_view = release_view.clone();
            spawn(move || {
                let view = histogram.read_view();
                view.assert_captured_pair_is_live_and_consistent();
                view_ready.store(true, Ordering::Release);
                while !release_view.load(Ordering::Acquire) {
                    thread::yield_now();
                    view.assert_captured_pair_is_live_and_consistent();
                }
                view.assert_captured_pair_is_live_and_consistent();
            })
        };

        while !view_ready.load(Ordering::Acquire) {
            thread::yield_now();
        }

        let resizer = {
            let histogram = histogram.clone();
            spawn(move || {
                histogram.resize_to_second_pair();
            })
        };

        thread::yield_now();
        release_view.store(true, Ordering::Release);

        reader.join().unwrap();
        resizer.join().unwrap();

        assert!(histogram.active_pair_is_second_pair());
        assert!(histogram.old_pair_is_retired());
    });
}

#[test]
fn read_view_allows_ordinary_writers_while_the_view_is_alive() {
    run_model(|| {
        let histogram = arc(ModelResizableConcurrentHistogram::new());
        let writer_done = arc(atomic_bool(false));

        let view = histogram.read_view();
        view.assert_captured_pair_is_live_and_consistent();

        let writer = {
            let histogram = histogram.clone();
            let writer_done = writer_done.clone();
            spawn(move || {
                histogram.record_one();
                writer_done.store(true, Ordering::Release);
            })
        };

        while !writer_done.load(Ordering::Acquire) {
            thread::yield_now();
            view.assert_captured_pair_is_live_and_consistent();
        }

        writer.join().unwrap();
        view.assert_captured_pair_is_live_and_consistent();
        drop(view);

        assert_eq!(12, histogram.total_count_all());
    });
}
