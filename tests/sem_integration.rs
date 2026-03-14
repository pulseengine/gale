//! Integration tests for the semaphore — exercises full API surface.
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
use gale::priority::Priority;
use gale::sem::{GiveResult, Semaphore, TakeResult};
use gale::thread::Thread;

fn make_running_thread(id: u32, prio: u32) -> Thread {
    let mut t = Thread::new(id, Priority::new(prio).unwrap());
    t.dispatch();
    t
}

// ==========================================================================
// P1: 0 <= count <= limit (always)
// ==========================================================================

#[test]
fn invariant_holds_after_init() {
    for limit in 1..=100 {
        for count in 0..=limit {
            let sem = Semaphore::init(count, limit).unwrap();
            assert!(sem.count_get() <= sem.limit_get());
        }
    }
}

#[test]
fn invariant_holds_after_repeated_give() {
    let mut sem = Semaphore::init(0, 10).unwrap();
    for _ in 0..1000 {
        sem.give();
        assert!(sem.count_get() <= sem.limit_get());
    }
}

#[test]
fn invariant_holds_after_repeated_take() {
    let mut sem = Semaphore::init(10, 10).unwrap();
    for _ in 0..1000 {
        sem.try_take();
        assert!(sem.count_get() <= sem.limit_get());
    }
}

// ==========================================================================
// P2: limit > 0 (always)
// ==========================================================================

#[test]
fn limit_always_positive() {
    assert!(Semaphore::init(0, 0).is_err());
    let sem = Semaphore::init(0, 1).unwrap();
    assert!(sem.limit_get() > 0);
}

// ==========================================================================
// P3: give with no waiters increments, capped at limit
// ==========================================================================

#[test]
fn give_increments_when_below_limit() {
    let mut sem = Semaphore::init(0, 5).unwrap();
    for expected in 1..=5 {
        match sem.give() {
            GiveResult::Incremented => {}
            other => panic!("expected Incremented, got {other:?}"),
        }
        assert_eq!(sem.count_get(), expected);
    }
}

#[test]
fn give_saturates_at_limit() {
    let mut sem = Semaphore::init(5, 5).unwrap();
    for _ in 0..100 {
        match sem.give() {
            GiveResult::Saturated => {}
            other => panic!("expected Saturated, got {other:?}"),
        }
        assert_eq!(sem.count_get(), 5);
    }
}

// ==========================================================================
// P4: give with waiters wakes highest-priority thread, count unchanged
// ==========================================================================

#[test]
fn give_wakes_highest_priority_thread() {
    let mut sem = Semaphore::init(0, 5).unwrap();
    // Block three threads: priorities 10, 2, 7
    sem.take_blocking(make_running_thread(1, 10));
    sem.take_blocking(make_running_thread(2, 2));
    sem.take_blocking(make_running_thread(3, 7));

    // Give should wake priority 2 first (thread id 2)
    match sem.give() {
        GiveResult::WokeThread(t) => {
            assert_eq!(t.id.id, 2);
            assert_eq!(t.return_value, OK);
        }
        other => panic!("expected WokeThread, got {other:?}"),
    }
    assert_eq!(sem.count_get(), 0); // count unchanged
}

// ==========================================================================
// P5: take when count > 0 decrements by exactly 1
// ==========================================================================

#[test]
fn take_decrements_by_one() {
    let mut sem = Semaphore::init(5, 5).unwrap();
    for expected in (0..5).rev() {
        assert_eq!(sem.try_take(), TakeResult::Acquired);
        assert_eq!(sem.count_get(), expected);
    }
}

// ==========================================================================
// P6: take when count == 0, no wait returns -EBUSY
// ==========================================================================

#[test]
fn take_empty_returns_wouldblock() {
    let mut sem = Semaphore::init(0, 5).unwrap();
    assert_eq!(sem.try_take(), TakeResult::WouldBlock);
    assert_eq!(sem.count_get(), 0);
}

// ==========================================================================
// P7: take when count == 0, with wait blocks thread
// ==========================================================================

#[test]
fn take_blocking_enqueues_thread() {
    let mut sem = Semaphore::init(0, 5).unwrap();
    let result = sem.take_blocking(make_running_thread(1, 5));
    assert!(result); // true = thread was enqueued in wait queue
    assert_eq!(sem.num_waiters(), 1);
    assert_eq!(sem.count_get(), 0);
}

// ==========================================================================
// P8: reset sets count to 0, wakes all waiters with -EAGAIN
// ==========================================================================

#[test]
fn reset_clears_everything() {
    let mut sem = Semaphore::init(0, 10).unwrap();
    sem.take_blocking(make_running_thread(1, 5));
    sem.take_blocking(make_running_thread(2, 3));
    sem.take_blocking(make_running_thread(3, 7));

    let woken = sem.reset();
    assert_eq!(woken, 3);
    assert_eq!(sem.count_get(), 0);
    assert_eq!(sem.num_waiters(), 0);
}

// ==========================================================================
// P9: no arithmetic overflow
// ==========================================================================

#[test]
fn no_overflow_at_u32_max_limit() {
    let mut sem = Semaphore::init(u32::MAX - 1, u32::MAX).unwrap();
    // Give should increment to u32::MAX
    sem.give();
    assert_eq!(sem.count_get(), u32::MAX);
    // Give should saturate, not overflow
    sem.give();
    assert_eq!(sem.count_get(), u32::MAX);
}

// ==========================================================================
// P10: wait queue ordering preserved
// ==========================================================================

#[test]
fn wait_queue_ordering_across_operations() {
    let mut sem = Semaphore::init(0, 5).unwrap();

    // Insert in non-sorted order
    sem.take_blocking(make_running_thread(1, 15));
    sem.take_blocking(make_running_thread(2, 3));
    sem.take_blocking(make_running_thread(3, 10));
    sem.take_blocking(make_running_thread(4, 1));
    sem.take_blocking(make_running_thread(5, 7));

    // Gives should return in priority order: 4, 2, 5, 3, 1
    let expected_ids = [4, 2, 5, 3, 1];
    for &expected_id in &expected_ids {
        match sem.give() {
            GiveResult::WokeThread(t) => assert_eq!(t.id.id, expected_id),
            other => panic!("expected WokeThread, got {other:?}"),
        }
    }
}
