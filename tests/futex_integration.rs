//! Integration tests for the futex — exercises full API surface.
//!
//! These tests run under: cargo test, miri, sanitizers.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::wildcard_enum_match_arm,
    clippy::shadow_unrelated,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use gale::error::*;
use gale::futex::{Futex, WaitResult};
use gale::priority::Priority;
use gale::thread::{Thread, ThreadState};

fn make_running_thread(id: u32, prio: u32) -> Thread {
    let mut t = Thread::new(id, Priority::new(prio).unwrap());
    t.dispatch();
    t
}

// ==========================================================================
// Initialization
// ==========================================================================

#[test]
fn init_sets_value_and_empty_queue() {
    let f = Futex::init(42);
    assert_eq!(f.val_get(), 42);
    assert_eq!(f.num_waiters(), 0);
}

#[test]
fn init_zero() {
    let f = Futex::init(0);
    assert_eq!(f.val_get(), 0);
    assert_eq!(f.num_waiters(), 0);
}

#[test]
fn init_max_value() {
    let f = Futex::init(u32::MAX);
    assert_eq!(f.val_get(), u32::MAX);
    assert_eq!(f.num_waiters(), 0);
}

// ==========================================================================
// FX1: wait only blocks when val == expected
// ==========================================================================

#[test]
fn fx1_wait_blocks_when_value_matches() {
    let mut f = Futex::init(10);
    let t = make_running_thread(1, 5);
    let result = f.wait(10, t);
    assert_eq!(result, WaitResult::Blocked);
    assert_eq!(f.num_waiters(), 1);
    assert_eq!(f.val_get(), 10); // value unchanged
}

#[test]
fn fx1_wait_blocks_at_zero() {
    let mut f = Futex::init(0);
    let t = make_running_thread(1, 5);
    let result = f.wait(0, t);
    assert_eq!(result, WaitResult::Blocked);
    assert_eq!(f.num_waiters(), 1);
}

// ==========================================================================
// FX2: wait with val != expected returns EAGAIN immediately
// ==========================================================================

#[test]
fn fx2_wait_mismatch_returns_immediately() {
    let mut f = Futex::init(10);
    let t = make_running_thread(1, 5);
    let result = f.wait(20, t);
    assert_eq!(result, WaitResult::Mismatch);
    assert_eq!(f.num_waiters(), 0); // not blocked
    assert_eq!(f.val_get(), 10); // value unchanged
}

#[test]
fn fx2_wait_mismatch_off_by_one() {
    let mut f = Futex::init(5);
    let t = make_running_thread(1, 5);
    let result = f.wait(4, t);
    assert_eq!(result, WaitResult::Mismatch);
    assert_eq!(f.num_waiters(), 0);
}

#[test]
fn fx2_wait_mismatch_zero_vs_nonzero() {
    let mut f = Futex::init(0);
    let t = make_running_thread(1, 5);
    let result = f.wait(1, t);
    assert_eq!(result, WaitResult::Mismatch);
    assert_eq!(f.num_waiters(), 0);
}

// ==========================================================================
// FX3: wake returns number of threads woken
// ==========================================================================

#[test]
fn fx3_wake_returns_count_zero() {
    let mut f = Futex::init(0);
    let result = f.wake(false);
    assert_eq!(result.woken, 0);
}

#[test]
fn fx3_wake_returns_count_one() {
    let mut f = Futex::init(0);
    f.wait(0, make_running_thread(1, 5));
    let result = f.wake(false);
    assert_eq!(result.woken, 1);
}

#[test]
fn fx3_wake_returns_count_all() {
    let mut f = Futex::init(0);
    f.wait(0, make_running_thread(1, 5));
    f.wait(0, make_running_thread(2, 3));
    f.wait(0, make_running_thread(3, 7));
    let result = f.wake(true);
    assert_eq!(result.woken, 3);
}

// ==========================================================================
// FX4: wake_all=false wakes at most 1
// ==========================================================================

#[test]
fn fx4_wake_one_from_empty() {
    let mut f = Futex::init(0);
    let result = f.wake(false);
    assert_eq!(result.woken, 0);
    assert_eq!(f.num_waiters(), 0);
}

#[test]
fn fx4_wake_one_from_multiple() {
    let mut f = Futex::init(0);
    f.wait(0, make_running_thread(1, 5));
    f.wait(0, make_running_thread(2, 3));
    f.wait(0, make_running_thread(3, 7));
    assert_eq!(f.num_waiters(), 3);

    let result = f.wake(false);
    assert_eq!(result.woken, 1);
    assert_eq!(f.num_waiters(), 2);
}

#[test]
fn fx4_wake_one_wakes_highest_priority() {
    let mut f = Futex::init(0);
    f.wait(0, make_running_thread(10, 15)); // low priority
    f.wait(0, make_running_thread(20, 1));  // highest priority
    f.wait(0, make_running_thread(30, 8));  // medium priority

    let result = f.wake(false);
    assert_eq!(result.woken, 1);
    // The woken thread should be thread 20 (highest priority = lowest value)
    let woken_thread = result.threads[0].as_ref().unwrap();
    assert_eq!(woken_thread.id.id, 20);
    assert_eq!(woken_thread.state, ThreadState::Ready);
    assert_eq!(woken_thread.return_value, OK);
}

// ==========================================================================
// FX5: wake_all=true wakes all
// ==========================================================================

#[test]
fn fx5_wake_all_empty() {
    let mut f = Futex::init(0);
    let result = f.wake(true);
    assert_eq!(result.woken, 0);
    assert_eq!(f.num_waiters(), 0);
}

#[test]
fn fx5_wake_all_single() {
    let mut f = Futex::init(0);
    f.wait(0, make_running_thread(1, 5));
    let result = f.wake(true);
    assert_eq!(result.woken, 1);
    assert_eq!(f.num_waiters(), 0);
}

#[test]
fn fx5_wake_all_multiple() {
    let mut f = Futex::init(0);
    for i in 0..10 {
        f.wait(0, make_running_thread(i, (i % 31) + 1));
    }
    assert_eq!(f.num_waiters(), 10);

    let result = f.wake(true);
    assert_eq!(result.woken, 10);
    assert_eq!(f.num_waiters(), 0);
}

#[test]
fn fx5_wake_all_threads_are_ready() {
    let mut f = Futex::init(0);
    f.wait(0, make_running_thread(1, 5));
    f.wait(0, make_running_thread(2, 3));
    f.wait(0, make_running_thread(3, 7));

    let result = f.wake(true);
    for i in 0..result.woken {
        let t = result.threads[i as usize].as_ref().unwrap();
        assert_eq!(t.state, ThreadState::Ready);
        assert_eq!(t.return_value, OK);
    }
}

// ==========================================================================
// FX6: no overflow in woken count
// ==========================================================================

#[test]
fn fx6_many_waiters_no_overflow() {
    let mut f = Futex::init(0);
    // Fill to near capacity
    for i in 0..60 {
        f.wait(0, make_running_thread(i, (i % 31) + 1));
    }
    assert_eq!(f.num_waiters(), 60);

    let result = f.wake(true);
    assert_eq!(result.woken, 60);
    assert_eq!(f.num_waiters(), 0);
}

// ==========================================================================
// Value operations
// ==========================================================================

#[test]
fn val_set_get_roundtrip() {
    let mut f = Futex::init(0);
    f.val_set(42);
    assert_eq!(f.val_get(), 42);
    f.val_set(0);
    assert_eq!(f.val_get(), 0);
    f.val_set(u32::MAX);
    assert_eq!(f.val_get(), u32::MAX);
}

#[test]
fn val_change_affects_wait_decision() {
    let mut f = Futex::init(0);
    // Wait with matching value -> blocks
    let result = f.wait(0, make_running_thread(1, 5));
    assert_eq!(result, WaitResult::Blocked);

    // Change value
    f.val_set(1);

    // Wait with old expected -> mismatch
    let result = f.wait(0, make_running_thread(2, 5));
    assert_eq!(result, WaitResult::Mismatch);

    // Wait with new expected -> blocks
    let result = f.wait(1, make_running_thread(3, 5));
    assert_eq!(result, WaitResult::Blocked);
}

// ==========================================================================
// Compositional: wait-wake roundtrip
// ==========================================================================

#[test]
fn wait_wake_roundtrip() {
    let mut f = Futex::init(0);
    assert_eq!(f.num_waiters(), 0);

    f.wait(0, make_running_thread(1, 5));
    assert_eq!(f.num_waiters(), 1);

    let result = f.wake(false);
    assert_eq!(result.woken, 1);
    assert_eq!(f.num_waiters(), 0);
}

#[test]
fn multiple_wait_wake_cycles() {
    let mut f = Futex::init(0);

    for cycle in 0..5 {
        let base_id = cycle * 3;
        // Add 3 waiters
        for i in 0..3 {
            f.wait(0, make_running_thread(base_id + i, (i + 1) * 3));
        }
        assert_eq!(f.num_waiters(), 3);

        // Wake all
        let result = f.wake(true);
        assert_eq!(result.woken, 3);
        assert_eq!(f.num_waiters(), 0);
    }
}

#[test]
fn wake_one_at_a_time() {
    let mut f = Futex::init(0);
    f.wait(0, make_running_thread(1, 10));
    f.wait(0, make_running_thread(2, 5));
    f.wait(0, make_running_thread(3, 1)); // highest priority

    // Wake one at a time, should be in priority order
    let r1 = f.wake(false);
    assert_eq!(r1.woken, 1);
    assert_eq!(r1.threads[0].as_ref().unwrap().id.id, 3);
    assert_eq!(f.num_waiters(), 2);

    let r2 = f.wake(false);
    assert_eq!(r2.woken, 1);
    assert_eq!(r2.threads[0].as_ref().unwrap().id.id, 2);
    assert_eq!(f.num_waiters(), 1);

    let r3 = f.wake(false);
    assert_eq!(r3.woken, 1);
    assert_eq!(r3.threads[0].as_ref().unwrap().id.id, 1);
    assert_eq!(f.num_waiters(), 0);

    // No more to wake
    let r4 = f.wake(false);
    assert_eq!(r4.woken, 0);
}

// ==========================================================================
// Edge cases
// ==========================================================================

#[test]
fn wake_after_value_change() {
    let mut f = Futex::init(10);
    f.wait(10, make_running_thread(1, 5));
    assert_eq!(f.num_waiters(), 1);

    // Changing the value does not affect already-blocked threads
    f.val_set(20);
    let result = f.wake(false);
    assert_eq!(result.woken, 1);
    assert_eq!(f.num_waiters(), 0);
}

#[test]
fn mixed_wait_results() {
    let mut f = Futex::init(5);

    // Match -> block
    let r1 = f.wait(5, make_running_thread(1, 5));
    assert_eq!(r1, WaitResult::Blocked);

    // Mismatch -> not blocked
    let r2 = f.wait(99, make_running_thread(2, 5));
    assert_eq!(r2, WaitResult::Mismatch);

    // Only 1 waiter (from the match)
    assert_eq!(f.num_waiters(), 1);
}

#[test]
fn wake_preserves_value() {
    let mut f = Futex::init(42);
    f.wait(42, make_running_thread(1, 5));
    f.wake(false);
    assert_eq!(f.val_get(), 42); // value not changed by wake
}

#[test]
fn wait_preserves_value() {
    let mut f = Futex::init(42);
    f.wait(42, make_running_thread(1, 5));
    assert_eq!(f.val_get(), 42); // value not changed by wait
    f.wait(99, make_running_thread(2, 5)); // mismatch
    assert_eq!(f.val_get(), 42); // still unchanged
}
