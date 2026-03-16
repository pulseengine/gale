//! Integration tests for the time-slicing model.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::timeslice::TimeSlice;

#[test]
fn init_disabled() {
    let ts = TimeSlice::init_disabled();
    assert_eq!(ts.remaining(), 0);
    assert_eq!(ts.max_ticks(), 0);
    assert!(!ts.is_expired());
    assert!(!ts.is_enabled());
}

#[test]
fn set_config_enables_and_resets() {
    let mut ts = TimeSlice::init_disabled();
    ts.set_config(100);
    assert!(ts.is_enabled());
    assert_eq!(ts.max_ticks(), 100);
    assert_eq!(ts.remaining(), 100);
    assert!(!ts.is_expired());
}

#[test]
fn set_config_zero_disables() {
    let mut ts = TimeSlice::init_disabled();
    ts.set_config(50);
    assert!(ts.is_enabled());

    ts.set_config(0);
    assert!(!ts.is_enabled());
    assert_eq!(ts.max_ticks(), 0);
    assert_eq!(ts.remaining(), 0);
}

#[test]
fn tick_decrements() {
    let mut ts = TimeSlice::init_disabled();
    ts.set_config(5);

    ts.tick();
    assert_eq!(ts.remaining(), 4);
    assert!(!ts.is_expired());

    ts.tick();
    assert_eq!(ts.remaining(), 3);
    assert!(!ts.is_expired());
}

#[test]
fn tick_to_zero_expires() {
    let mut ts = TimeSlice::init_disabled();
    ts.set_config(3);

    ts.tick(); // 2
    ts.tick(); // 1
    ts.tick(); // 0 -> expired
    assert_eq!(ts.remaining(), 0);
    assert!(ts.is_expired());
}

#[test]
fn tick_at_zero_no_underflow() {
    let mut ts = TimeSlice::init_disabled();
    ts.set_config(1);

    ts.tick(); // 0 -> expired
    assert_eq!(ts.remaining(), 0);
    assert!(ts.is_expired());

    // Tick again at 0 — should not underflow
    ts.tick();
    assert_eq!(ts.remaining(), 0);
    assert!(ts.is_expired());
}

#[test]
fn reset_restores_max() {
    let mut ts = TimeSlice::init_disabled();
    ts.set_config(10);

    ts.tick();
    ts.tick();
    ts.tick();
    assert_eq!(ts.remaining(), 7);

    ts.reset();
    assert_eq!(ts.remaining(), 10);
    assert!(!ts.is_expired());
}

#[test]
fn reset_clears_expired() {
    let mut ts = TimeSlice::init_disabled();
    ts.set_config(2);

    ts.tick();
    ts.tick();
    assert!(ts.is_expired());

    ts.reset();
    assert!(!ts.is_expired());
    assert_eq!(ts.remaining(), 2);
}

#[test]
fn consume_expired_reads_and_clears() {
    let mut ts = TimeSlice::init_disabled();
    ts.set_config(1);

    ts.tick();
    assert!(ts.is_expired());

    let was = ts.consume_expired();
    assert!(was);
    assert!(!ts.is_expired());

    // Second consume returns false
    let was2 = ts.consume_expired();
    assert!(!was2);
}

#[test]
fn full_countdown_cycle() {
    let mut ts = TimeSlice::init_disabled();
    ts.set_config(5);

    for i in (0..5).rev() {
        assert!(!ts.is_expired());
        ts.tick();
        assert_eq!(ts.remaining(), i);
    }
    assert!(ts.is_expired());

    // Reset and do it again
    ts.reset();
    assert!(!ts.is_expired());
    assert_eq!(ts.remaining(), 5);

    for i in (0..5).rev() {
        ts.tick();
        assert_eq!(ts.remaining(), i);
    }
    assert!(ts.is_expired());
}

#[test]
fn reconfigure_mid_slice() {
    let mut ts = TimeSlice::init_disabled();
    ts.set_config(10);

    ts.tick();
    ts.tick();
    assert_eq!(ts.remaining(), 8);

    // Reconfigure with different max
    ts.set_config(3);
    assert_eq!(ts.remaining(), 3);
    assert_eq!(ts.max_ticks(), 3);
    assert!(!ts.is_expired());
}

#[test]
fn max_ticks_value() {
    let mut ts = TimeSlice::init_disabled();
    ts.set_config(u32::MAX);
    assert_eq!(ts.max_ticks(), u32::MAX);
    assert_eq!(ts.remaining(), u32::MAX);
    assert!(ts.is_enabled());

    ts.tick();
    assert_eq!(ts.remaining(), u32::MAX - 1);
}

#[test]
fn single_tick_slice() {
    let mut ts = TimeSlice::init_disabled();
    ts.set_config(1);
    assert_eq!(ts.remaining(), 1);

    ts.tick();
    assert_eq!(ts.remaining(), 0);
    assert!(ts.is_expired());

    ts.reset();
    assert_eq!(ts.remaining(), 1);
    assert!(!ts.is_expired());
}

#[test]
fn disabled_tick_still_marks_expired() {
    let mut ts = TimeSlice::init_disabled();
    // Disabled: ticks = 0, max = 0
    ts.tick();
    // At 0 with no ticks, expired should be set
    assert!(ts.is_expired());
}

#[test]
fn clone_and_eq() {
    let mut ts1 = TimeSlice::init_disabled();
    ts1.set_config(50);
    let ts2 = ts1.clone();
    assert_eq!(ts1, ts2);

    ts1.tick();
    assert_ne!(ts1, ts2);
}

#[test]
fn invariant_across_operations() {
    let mut ts = TimeSlice::init_disabled();

    // TS1: bounds invariant
    assert!(ts.remaining() <= ts.max_ticks());

    ts.set_config(20);
    assert!(ts.remaining() <= ts.max_ticks());

    for _ in 0..20 {
        ts.tick();
        assert!(ts.remaining() <= ts.max_ticks());
    }

    ts.reset();
    assert!(ts.remaining() <= ts.max_ticks());
}
