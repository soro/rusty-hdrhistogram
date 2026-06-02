#[path = "support/loom_model.rs"]
mod loom_model;

use loom::sync::atomic::Ordering;
use loom::thread;
use loom_model::{arc, atomic_bool, atomic_usize, drop_guard, run_model, spawn, ModelPhaser};

#[test]
fn flip_observes_writer_publication() {
    run_model(|| {
        let phaser = arc(ModelPhaser::new());
        let payload = arc(atomic_usize(0));
        let writer_started = arc(atomic_bool(false));

        let writer = {
            let phaser = phaser.clone();
            let payload = payload.clone();
            let writer_started = writer_started.clone();
            spawn(move || {
                let guard = phaser.begin_writer_critical_section();
                writer_started.store(true, Ordering::Release);
                thread::yield_now();
                payload.store(1, Ordering::Relaxed);
                drop_guard(guard);
            })
        };

        while !writer_started.load(Ordering::Acquire) {
            thread::yield_now();
        }

        let reader = {
            let phaser = phaser.clone();
            let payload = payload.clone();
            spawn(move || {
                let guard = phaser.reader_lock();
                guard.flip();
                assert_eq!(1, payload.load(Ordering::Relaxed));
            })
        };

        writer.join().unwrap();
        reader.join().unwrap();
    });
}

#[test]
fn consecutive_flips_wait_for_the_correct_phase() {
    run_model(|| {
        let phaser = arc(ModelPhaser::new());
        let first_payload = arc(atomic_usize(0));
        let second_payload = arc(atomic_usize(0));

        {
            let guard = phaser.begin_writer_critical_section();
            first_payload.store(1, Ordering::Relaxed);
            drop_guard(guard);
        }

        let first_flip = phaser.reader_lock();
        first_flip.flip();
        assert_eq!(1, first_payload.load(Ordering::Relaxed));
        drop_guard(first_flip);

        let writer_started = arc(loom_model::atomic_bool(false));
        let writer = {
            let phaser = phaser.clone();
            let second_payload = second_payload.clone();
            let writer_started = writer_started.clone();
            spawn(move || {
                let guard = phaser.begin_writer_critical_section();
                writer_started.store(true, Ordering::Release);
                thread::yield_now();
                second_payload.store(1, Ordering::Relaxed);
                drop_guard(guard);
            })
        };

        while !writer_started.load(Ordering::Acquire) {
            thread::yield_now();
        }

        let second_flip = phaser.reader_lock();
        second_flip.flip();
        assert_eq!(1, second_payload.load(Ordering::Relaxed));
        drop_guard(second_flip);

        writer.join().unwrap();
    });
}
