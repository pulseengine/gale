//! Integration tests for the timer model.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::error::*;
use gale::timer::Timer;

#[test]
fn init_one_shot() {
    let t = Timer::init(0);
    assert_eq!(t.status, 0);
    assert_eq!(t.period_get(), 0);
    assert!(!t.is_running());
}

#[test]
fn init_periodic() {
    let t = Timer::init(100);
    assert_eq!(t.status, 0);
    assert_eq!(t.period_get(), 100);
    assert!(!t.is_running());
}

#[test]
fn start_sets_running_and_resets_status() {
    let mut t = Timer::init(50);
    // Simulate some expiries before start
    t.expire().unwrap();
    t.expire().unwrap();
    assert_eq!(t.status_peek(), 2);

    t.start();
    assert!(t.is_running());
    assert_eq!(t.status_peek(), 0);
}

#[test]
fn expire_increments_status() {
    let mut t = Timer::init(10);
    t.start();

    assert_eq!(t.expire(), Ok(1));
    assert_eq!(t.status_peek(), 1);

    assert_eq!(t.expire(), Ok(2));
    assert_eq!(t.status_peek(), 2);

    assert_eq!(t.expire(), Ok(3));
    assert_eq!(t.status_peek(), 3);
}

#[test]
fn expire_overflow_returns_error() {
    let mut t = Timer::init(0);
    t.status = u32::MAX;

    assert_eq!(t.expire(), Err(EOVERFLOW));
    assert_eq!(t.status_peek(), u32::MAX);
}

#[test]
fn expire_just_below_max() {
    let mut t = Timer::init(0);
    t.status = u32::MAX - 1;

    assert_eq!(t.expire(), Ok(u32::MAX));
    assert_eq!(t.status_peek(), u32::MAX);

    // Now at MAX, next expire should fail
    assert_eq!(t.expire(), Err(EOVERFLOW));
}

#[test]
fn status_get_reads_and_resets() {
    let mut t = Timer::init(10);
    t.start();

    t.expire().unwrap();
    t.expire().unwrap();
    t.expire().unwrap();

    assert_eq!(t.status_get(), 3);
    assert_eq!(t.status_peek(), 0);

    // Second read returns 0
    assert_eq!(t.status_get(), 0);
}

#[test]
fn stop_clears_running_and_status() {
    let mut t = Timer::init(10);
    t.start();
    t.expire().unwrap();
    t.expire().unwrap();

    t.stop();
    assert!(!t.is_running());
    assert_eq!(t.status_peek(), 0);
}

#[test]
fn periodic_vs_one_shot() {
    let one_shot = Timer::init(0);
    let periodic = Timer::init(42);

    assert_eq!(one_shot.period_get(), 0);
    assert!(periodic.period_get() > 0);
    assert_eq!(periodic.period_get(), 42);
}

#[test]
fn start_stop_start_cycle() {
    let mut t = Timer::init(100);

    // First cycle
    t.start();
    assert!(t.is_running());
    t.expire().unwrap();
    t.expire().unwrap();
    assert_eq!(t.status_peek(), 2);

    t.stop();
    assert!(!t.is_running());
    assert_eq!(t.status_peek(), 0);

    // Second cycle
    t.start();
    assert!(t.is_running());
    assert_eq!(t.status_peek(), 0);
    t.expire().unwrap();
    assert_eq!(t.status_peek(), 1);
}

#[test]
fn status_get_after_stop() {
    let mut t = Timer::init(10);
    t.start();
    t.expire().unwrap();
    t.expire().unwrap();
    t.stop();

    // stop already reset status, so get should return 0
    assert_eq!(t.status_get(), 0);
}

#[test]
fn many_expiries() {
    let mut t = Timer::init(1);
    t.start();

    for i in 1u32..=1000 {
        assert_eq!(t.expire(), Ok(i));
    }
    assert_eq!(t.status_peek(), 1000);
    assert_eq!(t.status_get(), 1000);
    assert_eq!(t.status_peek(), 0);
}

#[test]
fn clone_and_eq() {
    let t1 = Timer::init(50);
    let t2 = t1;
    assert_eq!(t1, t2);

    let mut t3 = t1;
    t3.start();
    assert_ne!(t1, t3);
}

#[test]
fn period_preserved_across_operations() {
    let mut t = Timer::init(42);
    assert_eq!(t.period_get(), 42);

    t.start();
    assert_eq!(t.period_get(), 42);

    t.expire().unwrap();
    assert_eq!(t.period_get(), 42);

    t.status_get();
    assert_eq!(t.period_get(), 42);

    t.stop();
    assert_eq!(t.period_get(), 42);
}
