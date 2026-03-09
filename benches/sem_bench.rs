//! Benchmarks for semaphore operations.
//!
//! Run with: cargo bench

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::wildcard_enum_match_arm,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use gale::priority::Priority;
use gale::sem::Semaphore;
use gale::thread::Thread;

fn make_running_thread(id: u32, prio: u32) -> Thread {
    let mut t = Thread::new(id, Priority::new(prio).unwrap());
    t.dispatch();
    t
}

fn bench_init(c: &mut Criterion) {
    c.bench_function("sem_init", |b| {
        b.iter(|| Semaphore::init(black_box(0), black_box(100)));
    });
}

fn bench_give_increment(c: &mut Criterion) {
    c.bench_function("sem_give_increment", |b| {
        let mut sem = Semaphore::init(0, u32::MAX).unwrap();
        b.iter(|| {
            sem.give();
        });
    });
}

fn bench_give_saturated(c: &mut Criterion) {
    c.bench_function("sem_give_saturated", |b| {
        let mut sem = Semaphore::init(100, 100).unwrap();
        b.iter(|| {
            sem.give();
        });
    });
}

fn bench_try_take(c: &mut Criterion) {
    c.bench_function("sem_try_take", |b| {
        let mut sem = Semaphore::init(u32::MAX, u32::MAX).unwrap();
        b.iter(|| {
            sem.try_take();
        });
    });
}

fn bench_give_take_cycle(c: &mut Criterion) {
    c.bench_function("sem_give_take_cycle", |b| {
        let mut sem = Semaphore::init(0, 100).unwrap();
        b.iter(|| {
            sem.give();
            sem.try_take();
        });
    });
}

fn bench_give_wake_thread(c: &mut Criterion) {
    c.bench_function("sem_give_wake_thread", |b| {
        b.iter(|| {
            let mut sem = Semaphore::init(0, 5).unwrap();
            let t = make_running_thread(1, 5);
            sem.take_blocking(t);
            sem.give();
        });
    });
}

fn bench_reset_with_waiters(c: &mut Criterion) {
    c.bench_function("sem_reset_10_waiters", |b| {
        b.iter(|| {
            let mut sem = Semaphore::init(0, 5).unwrap();
            for i in 0..10 {
                let t = make_running_thread(i, i % 32);
                sem.take_blocking(t);
            }
            sem.reset();
        });
    });
}

criterion_group!(
    benches,
    bench_init,
    bench_give_increment,
    bench_give_saturated,
    bench_try_take,
    bench_give_take_cycle,
    bench_give_wake_thread,
    bench_reset_with_waiters,
);
criterion_main!(benches);
