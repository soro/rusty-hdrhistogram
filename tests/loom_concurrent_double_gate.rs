#[path = "support/loom_model.rs"]
mod loom_model;

use loom::thread;
use loom_model::{arc, drop_guard, run_model, spawn, ModelConcurrentDoubleGate};

#[test]
fn pending_range_shift_does_not_close_gate_while_waiting_for_read_view() {
    run_model(|| {
        let histogram = arc(ModelConcurrentDoubleGate::new());
        let view = histogram.read_view();

        let shifting = {
            let histogram = histogram.clone();
            spawn(move || {
                histogram.shift_after_locking_phaser();
            })
        };

        thread::yield_now();

        let writer = {
            let histogram = histogram.clone();
            spawn(move || {
                assert!(histogram.try_record_one());
            })
        };

        writer.join().unwrap();
        assert_eq!(histogram.count(), 1);

        drop_guard(view);
        shifting.join().unwrap();
        assert_eq!(histogram.ratio(), 2);
    });
}

#[test]
fn range_shift_gate_excludes_writer_publication_during_mutation() {
    run_model(|| {
        let histogram = arc(ModelConcurrentDoubleGate::new());

        let shifting = {
            let histogram = histogram.clone();
            spawn(move || {
                histogram.shift_after_locking_phaser();
            })
        };

        let writer = {
            let histogram = histogram.clone();
            spawn(move || {
                let _ = histogram.try_record_one();
            })
        };

        writer.join().unwrap();
        shifting.join().unwrap();
        assert_eq!(histogram.ratio(), 2);
    });
}

#[test]
fn reset_gate_excludes_writer_publication_while_counts_are_cleared() {
    run_model(|| {
        let histogram = arc(ModelConcurrentDoubleGate::new());

        let reset = {
            let histogram = histogram.clone();
            spawn(move || {
                histogram.reset_after_locking_phaser();
            })
        };

        let writer = {
            let histogram = histogram.clone();
            spawn(move || {
                let _ = histogram.try_record_one_with_total();
            })
        };

        reset.join().unwrap();
        writer.join().unwrap();
        assert!(histogram.count_equals_total());
    });
}

#[test]
fn range_generation_rejects_stale_pre_gate_range_decisions() {
    run_model(|| {
        let histogram = arc(ModelConcurrentDoubleGate::new());

        let writer = {
            let histogram = histogram.clone();
            spawn(move || {
                let _ = histogram.try_record_value_after_range_decision(1);
            })
        };

        let shifting = {
            let histogram = histogram.clone();
            spawn(move || {
                histogram.shift_after_locking_phaser();
            })
        };

        writer.join().unwrap();
        shifting.join().unwrap();
        assert_eq!(histogram.ratio(), 2);
    });
}
