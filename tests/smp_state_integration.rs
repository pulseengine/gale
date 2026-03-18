//! Integration tests for the SMP state tracking model.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::error::*;
use gale::smp_state::*;

#[test]
fn init_valid() {
    let s = SmpState::init(4).unwrap();
    assert_eq!(s.active_get(), 1); // CPU 0 only
    assert_eq!(s.inactive_get(), 3);
    assert_eq!(s.max_cpus_get(), 4);
    assert_eq!(s.lock_count_get(), 0);
    assert!(!s.all_active());
    assert!(!s.is_locked());
}

#[test]
fn init_single_cpu() {
    let s = SmpState::init(1).unwrap();
    assert_eq!(s.active_get(), 1);
    assert_eq!(s.inactive_get(), 0);
    assert!(s.all_active());
}

#[test]
fn init_rejects_zero() {
    assert_eq!(SmpState::init(0), Err(EINVAL));
}

#[test]
fn init_rejects_too_many() {
    assert_eq!(SmpState::init(17), Err(EINVAL));
    assert_eq!(SmpState::init(u32::MAX), Err(EINVAL));
}

#[test]
fn init_max_cpus() {
    let s = SmpState::init(MAX_CPUS).unwrap();
    assert_eq!(s.max_cpus_get(), MAX_CPUS);
}

#[test]
fn start_cpu() {
    let mut s = SmpState::init(4).unwrap();
    assert_eq!(s.start_cpu(), OK);
    assert_eq!(s.active_get(), 2);
    assert_eq!(s.inactive_get(), 2);
}

#[test]
fn start_all_cpus() {
    let mut s = SmpState::init(4).unwrap();
    for i in 1..4 {
        assert_eq!(s.start_cpu(), OK);
        assert_eq!(s.active_get(), i + 1);
    }
    assert!(s.all_active());
}

#[test]
fn start_when_all_active_returns_ebusy() {
    let mut s = SmpState::init(2).unwrap();
    assert_eq!(s.start_cpu(), OK);
    assert!(s.all_active());
    assert_eq!(s.start_cpu(), EBUSY);
    assert_eq!(s.active_get(), 2);
}

#[test]
fn stop_cpu() {
    let mut s = SmpState::init(4).unwrap();
    s.start_cpu();
    s.start_cpu();
    assert_eq!(s.active_get(), 3);
    assert_eq!(s.stop_cpu(), OK);
    assert_eq!(s.active_get(), 2);
}

#[test]
fn stop_cpu0_fails() {
    let mut s = SmpState::init(4).unwrap();
    // Only CPU 0 is active
    assert_eq!(s.stop_cpu(), EINVAL);
    assert_eq!(s.active_get(), 1);
}

#[test]
fn start_stop_roundtrip() {
    let mut s = SmpState::init(4).unwrap();
    let original = s;
    assert_eq!(s.start_cpu(), OK);
    assert_eq!(s.stop_cpu(), OK);
    assert_eq!(s, original);
}

#[test]
fn resume_cpu_same_as_start() {
    let mut s1 = SmpState::init(4).unwrap();
    let mut s2 = SmpState::init(4).unwrap();
    assert_eq!(s1.start_cpu(), OK);
    assert_eq!(s2.resume_cpu(), OK);
    assert_eq!(s1, s2);
}

#[test]
fn global_lock_unlock() {
    let mut s = SmpState::init(4).unwrap();
    assert!(!s.is_locked());
    assert_eq!(s.global_lock(), OK);
    assert!(s.is_locked());
    assert_eq!(s.lock_count_get(), 1);
    assert_eq!(s.global_unlock(), OK);
    assert!(!s.is_locked());
    assert_eq!(s.lock_count_get(), 0);
}

#[test]
fn global_lock_reentrant() {
    let mut s = SmpState::init(4).unwrap();
    s.global_lock();
    s.global_lock();
    s.global_lock();
    assert_eq!(s.lock_count_get(), 3);
    s.global_unlock();
    assert_eq!(s.lock_count_get(), 2);
    assert!(s.is_locked());
}

#[test]
fn global_unlock_empty_returns_einval() {
    let mut s = SmpState::init(4).unwrap();
    assert_eq!(s.global_unlock(), EINVAL);
    assert_eq!(s.lock_count_get(), 0);
}

#[test]
fn clone_and_eq() {
    let s1 = SmpState::init(4).unwrap();
    let s2 = s1;
    assert_eq!(s1, s2);

    let mut s3 = s1;
    s3.start_cpu();
    assert_ne!(s1, s3);
}

#[test]
fn stress_start_stop_cycles() {
    let mut s = SmpState::init(8).unwrap();
    for _ in 0..50 {
        // Start all
        for _ in 1..8 {
            assert_eq!(s.start_cpu(), OK);
        }
        assert!(s.all_active());
        // Stop all except CPU 0
        for _ in 1..8 {
            assert_eq!(s.stop_cpu(), OK);
        }
        assert_eq!(s.active_get(), 1);
    }
}
