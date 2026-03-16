//! Integration tests for the spinlock discipline model.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::error::*;
use gale::spinlock::{MAX_NEST_DEPTH, SpinlockState};

#[test]
fn init_creates_unlocked() {
    let s = SpinlockState::init();
    assert!(s.is_free());
    assert!(!s.is_held());
    assert_eq!(s.nest_depth(), 0);
    assert_eq!(s.owner_get(), None);
    assert!(!s.irq_saved);
}

#[test]
fn acquire_free_lock() {
    let mut s = SpinlockState::init();
    assert_eq!(s.acquire(42), OK);
    assert!(s.is_held());
    assert!(s.is_owner(42));
    assert_eq!(s.nest_depth(), 1);
    assert!(s.irq_saved);
}

#[test]
fn acquire_held_lock_returns_ebusy() {
    let mut s = SpinlockState::init();
    s.acquire(1);
    assert_eq!(s.acquire(2), EBUSY);
    // State unchanged
    assert!(s.is_owner(1));
    assert_eq!(s.nest_depth(), 1);
}

#[test]
fn sl5_double_acquire_same_owner_returns_ebusy() {
    let mut s = SpinlockState::init();
    s.acquire(10);
    assert_eq!(s.acquire(10), EBUSY);
    assert_eq!(s.nest_depth(), 1);
}

#[test]
fn acquire_check_free() {
    let s = SpinlockState::init();
    assert!(s.acquire_check(99));
}

#[test]
fn acquire_check_held() {
    let mut s = SpinlockState::init();
    s.acquire(1);
    assert!(!s.acquire_check(1));
    assert!(!s.acquire_check(2));
}

#[test]
fn release_by_owner() {
    let mut s = SpinlockState::init();
    s.acquire(5);
    assert_eq!(s.release(5), OK);
    assert!(s.is_free());
    assert_eq!(s.nest_depth(), 0);
    assert!(!s.irq_saved);
}

#[test]
fn sl2_release_by_non_owner_returns_eperm() {
    let mut s = SpinlockState::init();
    s.acquire(1);
    assert_eq!(s.release(2), EPERM);
    // State unchanged
    assert!(s.is_owner(1));
    assert_eq!(s.nest_depth(), 1);
}

#[test]
fn release_unlocked_returns_eperm() {
    let mut s = SpinlockState::init();
    assert_eq!(s.release(1), EPERM);
    assert!(s.is_free());
}

#[test]
fn sl3_nested_acquire_increments_depth() {
    let mut s = SpinlockState::init();
    assert_eq!(s.acquire_nested(7), OK);
    assert_eq!(s.nest_depth(), 1);

    assert_eq!(s.acquire_nested(7), OK);
    assert_eq!(s.nest_depth(), 2);

    assert_eq!(s.acquire_nested(7), OK);
    assert_eq!(s.nest_depth(), 3);

    // Still owned by 7
    assert!(s.is_owner(7));
}

#[test]
fn sl3_nested_release_decrements_depth() {
    let mut s = SpinlockState::init();
    s.acquire_nested(1);
    s.acquire_nested(1);
    s.acquire_nested(1);
    assert_eq!(s.nest_depth(), 3);

    assert_eq!(s.release(1), OK);
    assert_eq!(s.nest_depth(), 2);
    assert!(s.is_held());

    assert_eq!(s.release(1), OK);
    assert_eq!(s.nest_depth(), 1);
    assert!(s.is_held());

    assert_eq!(s.release(1), OK);
    assert_eq!(s.nest_depth(), 0);
    assert!(s.is_free());
}

#[test]
fn nested_acquire_different_owner_returns_ebusy() {
    let mut s = SpinlockState::init();
    s.acquire_nested(1);
    assert_eq!(s.acquire_nested(2), EBUSY);
    assert!(s.is_owner(1));
}

#[test]
fn sl4_fully_released_at_zero() {
    let mut s = SpinlockState::init();
    // Acquire 5 times nested
    for _ in 0..5 {
        assert_eq!(s.acquire_nested(42), OK);
    }
    assert_eq!(s.nest_depth(), 5);

    // Release 4 times — still held
    for _ in 0..4 {
        assert_eq!(s.release(42), OK);
        assert!(s.is_held());
    }

    // Final release — fully unlocked
    assert_eq!(s.release(42), OK);
    assert!(s.is_free());
    assert_eq!(s.nest_depth(), 0);
}

#[test]
fn max_nesting_depth() {
    let mut s = SpinlockState::init();
    for _ in 0..MAX_NEST_DEPTH {
        assert_eq!(s.acquire_nested(1), OK);
    }
    assert_eq!(s.nest_depth(), MAX_NEST_DEPTH);

    // One more -> EBUSY
    assert_eq!(s.acquire_nested(1), EBUSY);
    assert_eq!(s.nest_depth(), MAX_NEST_DEPTH);
}

#[test]
fn acquire_release_roundtrip() {
    let mut s = SpinlockState::init();
    let original = s.clone();

    s.acquire(99);
    s.release(99);

    assert_eq!(s, original);
}

#[test]
fn nested_roundtrip() {
    let mut s = SpinlockState::init();
    s.acquire_nested(1);
    let after_first = s.clone();

    s.acquire_nested(1);
    s.release(1);

    assert_eq!(s, after_first);
}

#[test]
fn owner_get_returns_correct_tid() {
    let mut s = SpinlockState::init();
    assert_eq!(s.owner_get(), None);

    s.acquire(1234);
    assert_eq!(s.owner_get(), Some(1234));
}
