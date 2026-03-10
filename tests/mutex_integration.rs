//! Integration tests for the mutex — exercises full API surface.
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

use gale::error::*;
use gale::mutex::{LockResult, Mutex, UnlockResult};
use gale::priority::Priority;
use gale::thread::{Thread, ThreadState};

fn make_running_thread(id: u32, prio: u32) -> Thread {
    let mut t = Thread::new(id, Priority::new(prio).unwrap());
    t.dispatch();
    t
}

// ==========================================================================
// M1: lock_count > 0 ⟺ owner.is_some() (always)
// ==========================================================================

#[test]
fn m1_invariant_after_init() {
    let m = Mutex::init();
    assert_eq!(m.lock_count_get(), 0);
    assert!(m.owner_get().is_none());
}

#[test]
fn m1_invariant_after_lock() {
    let mut m = Mutex::init();
    m.try_lock(1);
    assert!(m.lock_count_get() > 0);
    assert!(m.owner_get().is_some());
}

#[test]
fn m1_invariant_after_full_unlock() {
    let mut m = Mutex::init();
    m.try_lock(1);
    m.unlock(1).unwrap();
    assert_eq!(m.lock_count_get(), 0);
    assert!(m.owner_get().is_none());
}

// ==========================================================================
// M2: wait_q non-empty ⟹ mutex is locked
// ==========================================================================

#[test]
fn m2_waiters_implies_locked() {
    let mut m = Mutex::init();
    m.try_lock(1);
    m.lock_blocking(make_running_thread(2, 5));
    assert!(m.num_waiters() > 0);
    assert!(m.is_locked());
}

// ==========================================================================
// M3: try_lock when unlocked: owner set, lock_count = 1
// ==========================================================================

#[test]
fn m3_lock_unlocked_mutex() {
    let mut m = Mutex::init();
    assert_eq!(m.try_lock(42), LockResult::Acquired);
    assert_eq!(m.owner_get(), Some(42));
    assert_eq!(m.lock_count_get(), 1);
}

// ==========================================================================
// M4: try_lock when locked by same thread: reentrant
// ==========================================================================

#[test]
fn m4_reentrant_lock() {
    let mut m = Mutex::init();
    m.try_lock(1);
    for depth in 2..=10 {
        assert_eq!(m.try_lock(1), LockResult::Acquired);
        assert_eq!(m.lock_count_get(), depth);
        assert_eq!(m.owner_get(), Some(1));
    }
}

// ==========================================================================
// M5: try_lock by different thread: returns WouldBlock
// ==========================================================================

#[test]
fn m5_lock_contention() {
    let mut m = Mutex::init();
    m.try_lock(1);
    assert_eq!(m.try_lock(2), LockResult::WouldBlock);
    assert_eq!(m.try_lock(3), LockResult::WouldBlock);
    // State unchanged
    assert_eq!(m.owner_get(), Some(1));
    assert_eq!(m.lock_count_get(), 1);
}

// ==========================================================================
// M6: unlock by non-owner: returns error
// ==========================================================================

#[test]
fn m6a_unlock_not_locked() {
    let mut m = Mutex::init();
    assert!(matches!(m.unlock(1), Err(EINVAL)));
}

#[test]
fn m6b_unlock_not_owner() {
    let mut m = Mutex::init();
    m.try_lock(1);
    assert!(matches!(m.unlock(2), Err(EPERM)));
    // State unchanged
    assert_eq!(m.owner_get(), Some(1));
    assert_eq!(m.lock_count_get(), 1);
}

// ==========================================================================
// M7: unlock when lock_count > 1: decremented, owner unchanged
// ==========================================================================

#[test]
fn m7_reentrant_unlock() {
    let mut m = Mutex::init();
    m.try_lock(1);
    m.try_lock(1);
    m.try_lock(1);

    assert!(matches!(m.unlock(1), Ok(UnlockResult::Released)));
    assert_eq!(m.lock_count_get(), 2);
    assert_eq!(m.owner_get(), Some(1));
}

// ==========================================================================
// M8: unlock when lock_count == 1, waiter: ownership transferred
// ==========================================================================

#[test]
fn m8_ownership_transfer() {
    let mut m = Mutex::init();
    m.try_lock(1);
    m.lock_blocking(make_running_thread(2, 5));
    m.lock_blocking(make_running_thread(3, 3)); // higher priority

    match m.unlock(1) {
        Ok(UnlockResult::Transferred(t)) => {
            assert_eq!(t.id, 3); // highest priority woken first
            assert_eq!(t.state, ThreadState::Ready);
            assert_eq!(t.return_value, OK);
        }
        other => panic!("expected Transferred, got {other:?}"),
    }
    assert_eq!(m.owner_get(), Some(3));
    assert_eq!(m.lock_count_get(), 1);
    assert_eq!(m.num_waiters(), 1); // thread 2 still waiting
}

// ==========================================================================
// M9: unlock when lock_count == 1, no waiter: fully unlocked
// ==========================================================================

#[test]
fn m9_full_unlock() {
    let mut m = Mutex::init();
    m.try_lock(1);
    assert!(matches!(m.unlock(1), Ok(UnlockResult::Unlocked)));
    assert!(!m.is_locked());
    assert_eq!(m.lock_count_get(), 0);
    assert_eq!(m.owner_get(), None);
}

// ==========================================================================
// M10: no arithmetic overflow in lock_count
// ==========================================================================

#[test]
fn m10_no_overflow() {
    // The Verus code requires lock_count < u32::MAX as precondition.
    // The plain code uses checked_add. Verify it doesn't panic.
    let mut m = Mutex::init();
    m.try_lock(1);
    // Lock many times to exercise the addition
    for _ in 0..1000 {
        m.try_lock(1);
    }
    assert_eq!(m.lock_count_get(), 1001);
}

// ==========================================================================
// M11: wait queue ordering preserved
// ==========================================================================

#[test]
fn m11_priority_ordering() {
    let mut m = Mutex::init();
    m.try_lock(1);

    // Block threads in non-priority order
    m.lock_blocking(make_running_thread(10, 15));
    m.lock_blocking(make_running_thread(20, 3));
    m.lock_blocking(make_running_thread(30, 8));
    m.lock_blocking(make_running_thread(40, 1)); // highest priority

    // Transfers should be in priority order: 40, 20, 30, 10
    let expected = [40, 20, 30, 10];
    for (i, &expected_id) in expected.iter().enumerate() {
        let current_owner = m.owner_get().unwrap();
        match m.unlock(current_owner) {
            Ok(UnlockResult::Transferred(t)) => {
                assert_eq!(t.id, expected_id, "transfer {i}: expected thread {expected_id}");
            }
            other => panic!("transfer {i}: expected Transferred, got {other:?}"),
        }
    }
    // Last unlock should fully release
    let current_owner = m.owner_get().unwrap();
    assert!(matches!(m.unlock(current_owner), Ok(UnlockResult::Unlocked)));
}

// ==========================================================================
// Compositional: lock-unlock roundtrip
// ==========================================================================

#[test]
fn lock_unlock_roundtrip() {
    let mut m = Mutex::init();
    for thread_id in 1..=5 {
        assert_eq!(m.try_lock(thread_id), LockResult::Acquired);
        m.unlock(thread_id).unwrap();
        assert!(!m.is_locked());
    }
}

#[test]
fn reentrant_full_unwind() {
    let mut m = Mutex::init();
    let depth = 20;
    for _ in 0..depth {
        m.try_lock(1);
    }
    assert_eq!(m.lock_count_get(), depth);

    for remaining in (1..depth).rev() {
        m.unlock(1).unwrap();
        assert_eq!(m.lock_count_get(), remaining);
        assert!(m.is_locked());
    }
    m.unlock(1).unwrap();
    assert!(!m.is_locked());
}
