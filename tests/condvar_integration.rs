//! Integration tests for the condition variable — exercises full API surface.
//!
//! These tests run under: cargo test, miri, sanitizers.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::wildcard_enum_match_arm,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use gale::condvar::{CondVar, SignalResult};
use gale::error::*;
use gale::priority::Priority;
use gale::thread::{Thread, ThreadState};

fn make_running_thread(id: u32, prio: u32) -> Thread {
    let mut t = Thread::new(id, Priority::new(prio).unwrap());
    t.dispatch();
    t
}

// ==========================================================================
// C1: After init, wait queue is empty
// ==========================================================================

#[test]
fn c1_init_empty() {
    let cv = CondVar::init();
    assert_eq!(cv.num_waiters(), 0);
    assert!(!cv.has_waiters());
}

// ==========================================================================
// C2: Signal wakes at most one waiter (highest priority)
// ==========================================================================

#[test]
fn c2_signal_wakes_highest_priority() {
    let mut cv = CondVar::init();
    cv.wait_blocking(make_running_thread(1, 10));
    cv.wait_blocking(make_running_thread(2, 3));  // highest priority
    cv.wait_blocking(make_running_thread(3, 7));

    match cv.signal() {
        SignalResult::Woke(t) => {
            assert_eq!(t.id, 2);
            assert_eq!(t.state, ThreadState::Ready);
            assert_eq!(t.return_value, OK);
        }
        SignalResult::Empty => panic!("expected Woke"),
    }
    assert_eq!(cv.num_waiters(), 2);
}

// ==========================================================================
// C3: Signal on empty condvar is a no-op
// ==========================================================================

#[test]
fn c3_signal_empty_noop() {
    let mut cv = CondVar::init();
    assert!(matches!(cv.signal(), SignalResult::Empty));
    assert_eq!(cv.num_waiters(), 0);
}

// ==========================================================================
// C4: Broadcast wakes all waiters, returns woken count
// ==========================================================================

#[test]
fn c4_broadcast_wakes_all() {
    let mut cv = CondVar::init();
    cv.wait_blocking(make_running_thread(1, 5));
    cv.wait_blocking(make_running_thread(2, 3));
    cv.wait_blocking(make_running_thread(3, 8));
    cv.wait_blocking(make_running_thread(4, 1));

    let woken = cv.broadcast();
    assert_eq!(woken, 4);
    assert_eq!(cv.num_waiters(), 0);
    assert!(!cv.has_waiters());
}

// ==========================================================================
// C5: Broadcast on empty condvar returns 0
// ==========================================================================

#[test]
fn c5_broadcast_empty_returns_zero() {
    let mut cv = CondVar::init();
    assert_eq!(cv.broadcast(), 0);
    assert_eq!(cv.num_waiters(), 0);
}

// ==========================================================================
// C6: Wait adds thread to wait queue
// ==========================================================================

#[test]
fn c6_wait_adds_thread() {
    let mut cv = CondVar::init();
    assert!(cv.wait_blocking(make_running_thread(1, 5)));
    assert_eq!(cv.num_waiters(), 1);
    assert!(cv.has_waiters());
}

#[test]
fn c6_wait_multiple_threads() {
    let mut cv = CondVar::init();
    for i in 0..10 {
        assert!(cv.wait_blocking(make_running_thread(i, i % 32)));
    }
    assert_eq!(cv.num_waiters(), 10);
}

// ==========================================================================
// C7: Signal/broadcast preserve wait queue ordering
// ==========================================================================

#[test]
fn c7_signal_preserves_priority_order() {
    let mut cv = CondVar::init();
    cv.wait_blocking(make_running_thread(10, 15));
    cv.wait_blocking(make_running_thread(20, 3));
    cv.wait_blocking(make_running_thread(30, 8));
    cv.wait_blocking(make_running_thread(40, 1)); // highest priority

    // Should wake in priority order: 40, 20, 30, 10
    let expected = [40, 20, 30, 10];
    for (i, &expected_id) in expected.iter().enumerate() {
        match cv.signal() {
            SignalResult::Woke(t) => {
                assert_eq!(t.id, expected_id, "signal {i}: expected thread {expected_id}");
            }
            SignalResult::Empty => panic!("signal {i}: expected Woke, got Empty"),
        }
    }
    assert!(matches!(cv.signal(), SignalResult::Empty));
}

// ==========================================================================
// Compositional tests
// ==========================================================================

#[test]
fn signal_then_broadcast_drains() {
    let mut cv = CondVar::init();
    cv.wait_blocking(make_running_thread(1, 5));
    cv.wait_blocking(make_running_thread(2, 3));
    cv.wait_blocking(make_running_thread(3, 8));

    cv.signal(); // removes one
    assert_eq!(cv.num_waiters(), 2);

    let woken = cv.broadcast(); // removes rest
    assert_eq!(woken, 2);
    assert_eq!(cv.num_waiters(), 0);
}

#[test]
fn reuse_after_broadcast() {
    let mut cv = CondVar::init();
    cv.wait_blocking(make_running_thread(1, 5));
    cv.broadcast();

    // Condvar can be reused
    cv.wait_blocking(make_running_thread(2, 3));
    assert_eq!(cv.num_waiters(), 1);
    assert!(matches!(cv.signal(), SignalResult::Woke(_)));
    assert_eq!(cv.num_waiters(), 0);
}

#[test]
fn n_signals_equivalent_to_broadcast() {
    // Signal N times == broadcast on N waiters
    let n = 8;

    // Method 1: N signals
    let mut cv1 = CondVar::init();
    for i in 0..n {
        cv1.wait_blocking(make_running_thread(i, (i % 32)));
    }
    for _ in 0..n {
        cv1.signal();
    }
    assert_eq!(cv1.num_waiters(), 0);

    // Method 2: one broadcast
    let mut cv2 = CondVar::init();
    for i in 0..n {
        cv2.wait_blocking(make_running_thread(i, (i % 32)));
    }
    let woken = cv2.broadcast();
    assert_eq!(woken as u32, n);
    assert_eq!(cv2.num_waiters(), 0);
}

#[test]
fn broadcast_idempotent() {
    let mut cv = CondVar::init();
    assert_eq!(cv.broadcast(), 0);
    assert_eq!(cv.broadcast(), 0);
    assert_eq!(cv.broadcast(), 0);
    assert_eq!(cv.num_waiters(), 0);
}
