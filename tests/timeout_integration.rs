//! Integration tests for the timeout model.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::error::*;
use gale::timeout::{K_FOREVER_TICKS, K_NO_WAIT_TICKS, Timeout};

#[test]
fn init_creates_inactive_timeout() {
    let t = Timeout::init(0);
    assert!(!t.is_active());
    assert_eq!(t.now(), 0);
    assert_eq!(t.deadline, 0);
}

#[test]
fn init_with_nonzero_tick() {
    let t = Timeout::init(1000);
    assert!(!t.is_active());
    assert_eq!(t.now(), 1000);
}

#[test]
fn add_relative_sets_deadline() {
    let mut t = Timeout::init(100);
    let deadline = t.add(50).unwrap();
    assert_eq!(deadline, 150);
    assert!(t.is_active());
    assert_eq!(t.deadline, 150);
}

#[test]
fn add_relative_zero_duration() {
    let mut t = Timeout::init(100);
    let deadline = t.add(0).unwrap();
    assert_eq!(deadline, 100);
    assert!(t.is_active());
}

#[test]
fn add_relative_overflow_returns_einval() {
    let mut t = Timeout::init(100);
    // K_FOREVER_TICKS - 100 would exactly reach u64::MAX, which is reserved
    let result = t.add(K_FOREVER_TICKS - 100);
    assert_eq!(result, Err(EINVAL));
    assert!(!t.is_active());
}

#[test]
fn add_absolute_sets_deadline() {
    let mut t = Timeout::init(100);
    let deadline = t.add_absolute(200).unwrap();
    assert_eq!(deadline, 200);
    assert!(t.is_active());
}

#[test]
fn add_absolute_rejects_past() {
    let mut t = Timeout::init(100);
    assert_eq!(t.add_absolute(50), Err(EINVAL));
    assert!(!t.is_active());
}

#[test]
fn add_absolute_rejects_forever() {
    let mut t = Timeout::init(100);
    assert_eq!(t.add_absolute(K_FOREVER_TICKS), Err(EINVAL));
    assert!(!t.is_active());
}

#[test]
fn add_absolute_at_current_tick() {
    let mut t = Timeout::init(100);
    let deadline = t.add_absolute(100).unwrap();
    assert_eq!(deadline, 100);
    assert!(t.is_active());
}

#[test]
fn add_forever_creates_forever_timeout() {
    let mut t = Timeout::init(500);
    let forever = t.add_forever();
    assert!(forever.is_active());
    assert!(forever.is_forever());
    assert_eq!(forever.deadline, K_FOREVER_TICKS);
    assert_eq!(forever.now(), 500);
}

#[test]
fn add_no_wait_creates_immediate_timeout() {
    let mut t = Timeout::init(500);
    let immediate = t.add_no_wait();
    assert!(immediate.is_active());
    assert!(immediate.is_no_wait());
    assert_eq!(immediate.deadline, K_NO_WAIT_TICKS);
}

#[test]
fn abort_active_returns_ok() {
    let mut t = Timeout::init(100);
    t.add(50).unwrap();
    assert!(t.is_active());

    assert_eq!(t.abort(), OK);
    assert!(!t.is_active());
}

#[test]
fn abort_inactive_returns_einval() {
    let mut t = Timeout::init(100);
    assert_eq!(t.abort(), EINVAL);
    assert!(!t.is_active());
}

#[test]
fn announce_fires_expired_timeout() {
    let mut t = Timeout::init(0);
    t.add(10).unwrap();
    assert_eq!(t.deadline, 10);

    // Advance past deadline
    let fired = t.announce(15).unwrap();
    assert!(fired);
    assert!(!t.is_active());
    assert_eq!(t.now(), 15);
}

#[test]
fn announce_fires_at_exact_deadline() {
    let mut t = Timeout::init(0);
    t.add(10).unwrap();

    let fired = t.announce(10).unwrap();
    assert!(fired);
    assert!(!t.is_active());
}

#[test]
fn announce_does_not_fire_before_deadline() {
    let mut t = Timeout::init(0);
    t.add(10).unwrap();

    let fired = t.announce(5).unwrap();
    assert!(!fired);
    assert!(t.is_active());
    assert_eq!(t.now(), 5);
    assert_eq!(t.remaining(), 5);
}

#[test]
fn announce_does_not_fire_inactive() {
    let mut t = Timeout::init(0);
    let fired = t.announce(100).unwrap();
    assert!(!fired);
    assert_eq!(t.now(), 100);
}

#[test]
fn announce_does_not_fire_forever() {
    let mut t = Timeout::init(0);
    let forever = t.add_forever();
    let mut t2 = forever;
    let fired = t2.announce(1000).unwrap();
    assert!(!fired);
    assert!(t2.is_active());
    assert_eq!(t2.now(), 1000);
}

#[test]
fn announce_overflow_returns_einval() {
    let mut t = Timeout::init(100);
    t.add(50).unwrap();
    let result = t.announce(K_FOREVER_TICKS - 50);
    assert_eq!(result, Err(EINVAL));
    // State unchanged on error
    assert!(t.is_active());
    assert_eq!(t.now(), 100);
}

#[test]
fn remaining_active_timeout() {
    let mut t = Timeout::init(100);
    t.add(50).unwrap();
    assert_eq!(t.remaining(), 50);
}

#[test]
fn remaining_inactive_timeout() {
    let t = Timeout::init(100);
    assert_eq!(t.remaining(), 0);
}

#[test]
fn remaining_forever_timeout() {
    let mut t = Timeout::init(100);
    let forever = t.add_forever();
    assert_eq!(forever.remaining(), K_FOREVER_TICKS);
}

#[test]
fn remaining_after_partial_advance() {
    let mut t = Timeout::init(0);
    t.add(100).unwrap();
    t.announce(30).unwrap();
    assert_eq!(t.remaining(), 70);
}

#[test]
fn remaining_expired_returns_zero() {
    let mut t = Timeout::init(0);
    t.add(10).unwrap();
    t.announce(20).unwrap();
    // Now inactive after firing
    assert_eq!(t.remaining(), 0);
}

#[test]
fn expires_active() {
    let mut t = Timeout::init(100);
    t.add(50).unwrap();
    assert_eq!(t.expires(), 150);
}

#[test]
fn expires_inactive() {
    let t = Timeout::init(100);
    assert_eq!(t.expires(), 100); // returns current_tick when inactive
}

#[test]
fn timepoint_calc_basic() {
    let t = Timeout::init(100);
    let tp = t.timepoint_calc(50).unwrap();
    assert_eq!(tp, 150);
}

#[test]
fn timepoint_calc_overflow_returns_einval() {
    let t = Timeout::init(100);
    let result = t.timepoint_calc(K_FOREVER_TICKS - 50);
    assert_eq!(result, Err(EINVAL));
}

#[test]
fn timepoint_timeout_basic() {
    let t = Timeout::init(100);
    let remaining = t.timepoint_timeout(200);
    assert_eq!(remaining, 100);
}

#[test]
fn timepoint_timeout_past_returns_zero() {
    let t = Timeout::init(200);
    let remaining = t.timepoint_timeout(100);
    assert_eq!(remaining, 0);
}

#[test]
fn timepoint_timeout_forever() {
    let t = Timeout::init(100);
    assert_eq!(t.timepoint_timeout(K_FOREVER_TICKS), K_FOREVER_TICKS);
}

#[test]
fn timepoint_timeout_no_wait() {
    let t = Timeout::init(100);
    assert_eq!(t.timepoint_timeout(K_NO_WAIT_TICKS), 0);
}

#[test]
fn timepoint_roundtrip() {
    let t = Timeout::init(100);
    let duration = 50u64;
    let tp = t.timepoint_calc(duration).unwrap();
    let back = t.timepoint_timeout(tp);
    assert_eq!(back, duration);
}

#[test]
fn abort_then_re_add() {
    let mut t = Timeout::init(100);
    t.add(50).unwrap();
    assert!(t.is_active());

    t.abort();
    assert!(!t.is_active());

    // Re-schedule
    let deadline = t.add(30).unwrap();
    assert_eq!(deadline, 130);
    assert!(t.is_active());
}

#[test]
fn multiple_announce_steps() {
    let mut t = Timeout::init(0);
    t.add(100).unwrap();

    // Step 1: advance 30, not fired
    assert!(!t.announce(30).unwrap());
    assert_eq!(t.now(), 30);
    assert_eq!(t.remaining(), 70);

    // Step 2: advance 40, not fired
    assert!(!t.announce(40).unwrap());
    assert_eq!(t.now(), 70);
    assert_eq!(t.remaining(), 30);

    // Step 3: advance 30, exactly at deadline
    assert!(t.announce(30).unwrap());
    assert_eq!(t.now(), 100);
    assert!(!t.is_active());
}

#[test]
fn clone_and_eq() {
    let t1 = Timeout::init(42);
    let t2 = t1.clone();
    assert_eq!(t1, t2);

    let mut t3 = t1.clone();
    t3.add(10).unwrap();
    assert_ne!(t1, t3);
}

#[test]
fn large_tick_values() {
    // Test with large but valid tick values
    let large_tick = u64::MAX / 2;
    let mut t = Timeout::init(large_tick);
    assert_eq!(t.now(), large_tick);

    // Can still add a small duration
    let deadline = t.add(100).unwrap();
    assert_eq!(deadline, large_tick + 100);
}

#[test]
fn announce_no_wait_fires_immediately() {
    let mut t = Timeout::init(100);
    let immediate = t.add_no_wait();
    let mut t2 = immediate;
    // Any positive advance should fire a deadline-0 timeout
    let fired = t2.announce(1).unwrap();
    assert!(fired);
    assert!(!t2.is_active());
}

#[test]
fn announce_zero_ticks_no_wait() {
    let mut t = Timeout::init(100);
    let immediate = t.add_no_wait();
    let mut t2 = immediate;
    // Advance 0 ticks: deadline 0 <= current_tick 100, should fire
    let fired = t2.announce(0).unwrap();
    assert!(fired);
    assert!(!t2.is_active());
}
