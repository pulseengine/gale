//! Property-based tests for the futex.
//!
//! Uses proptest to generate random operation sequences and verify
//! that invariants are maintained.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::wildcard_enum_match_arm,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use gale::futex::{Futex, WaitResult};
use gale::priority::Priority;
use gale::thread::Thread;
use proptest::prelude::*;

/// Operations that can be performed on a futex.
#[derive(Debug, Clone)]
enum FutexOp {
    Wait {
        thread_id: u32,
        priority: u32,
        expected: u32,
    },
    WakeOne,
    WakeAll,
    ValSet {
        new_val: u32,
    },
    ValGet,
    NumWaiters,
}

fn futex_op_strategy() -> impl Strategy<Value = FutexOp> {
    prop_oneof![
        (0u32..1000, 1u32..32, 0u32..10).prop_map(|(id, prio, exp)| FutexOp::Wait {
            thread_id: id,
            priority: prio,
            expected: exp,
        }),
        Just(FutexOp::WakeOne),
        Just(FutexOp::WakeAll),
        (0u32..10).prop_map(|v| FutexOp::ValSet { new_val: v }),
        Just(FutexOp::ValGet),
        Just(FutexOp::NumWaiters),
    ]
}

proptest! {
    /// Invariant: num_waiters is consistent after any sequence of operations.
    #[test]
    fn invariant_holds_under_random_ops(
        initial_val in 0u32..10,
        ops in prop::collection::vec(futex_op_strategy(), 0..200),
    ) {
        let mut f = Futex::init(initial_val);
        let mut expected_waiters: u32 = 0;
        let mut next_tid: u32 = 0;

        for op in ops {
            match op {
                FutexOp::Wait { thread_id: _, priority, expected } => {
                    if expected_waiters < 60 {
                        // Use unique thread IDs to avoid duplicate-in-queue issues
                        next_tid += 1;
                        let p = match Priority::new(priority) {
                            Some(p) => p,
                            None => continue,
                        };
                        let mut t = Thread::new(next_tid, p);
                        t.dispatch();
                        let result = f.wait(expected, t);
                        match result {
                            WaitResult::Blocked => {
                                expected_waiters += 1;
                            }
                            WaitResult::Mismatch => {
                                // No change
                            }
                        }
                    }
                }
                FutexOp::WakeOne => {
                    let result = f.wake(false);
                    if expected_waiters > 0 && result.woken > 0 {
                        expected_waiters -= result.woken;
                    }
                }
                FutexOp::WakeAll => {
                    let result = f.wake(true);
                    expected_waiters -= result.woken;
                }
                FutexOp::ValSet { new_val } => {
                    f.val_set(new_val);
                }
                FutexOp::ValGet => {
                    f.val_get();
                }
                FutexOp::NumWaiters => {
                    f.num_waiters();
                }
            }

            // Check consistency
            prop_assert_eq!(
                f.num_waiters(), expected_waiters,
                "Waiter count mismatch: actual={}, expected={}",
                f.num_waiters(), expected_waiters
            );
        }
    }

    /// FX1/FX2: wait blocks iff val == expected.
    #[test]
    fn wait_blocks_iff_match(
        val in 0u32..100,
        expected in 0u32..100,
    ) {
        let mut f = Futex::init(val);
        let mut t = Thread::new(1, Priority::new(5).unwrap());
        t.dispatch();

        let result = f.wait(expected, t);
        if val == expected {
            prop_assert_eq!(result, WaitResult::Blocked);
            prop_assert_eq!(f.num_waiters(), 1);
        } else {
            prop_assert_eq!(result, WaitResult::Mismatch);
            prop_assert_eq!(f.num_waiters(), 0);
        }
        // Value never changes
        prop_assert_eq!(f.val_get(), val);
    }

    /// FX4: wake(false) wakes at most 1.
    #[test]
    fn wake_one_at_most_one(
        num_waiters in 0u32..30,
    ) {
        let mut f = Futex::init(0);
        for i in 0..num_waiters {
            let mut t = Thread::new(i, Priority::new((i % 31) + 1).unwrap());
            t.dispatch();
            f.wait(0, t);
        }
        prop_assert_eq!(f.num_waiters(), num_waiters);

        let result = f.wake(false);
        prop_assert!(result.woken <= 1);
        if num_waiters > 0 {
            prop_assert_eq!(result.woken, 1);
            prop_assert_eq!(f.num_waiters(), num_waiters - 1);
        } else {
            prop_assert_eq!(result.woken, 0);
            prop_assert_eq!(f.num_waiters(), 0);
        }
    }

    /// FX5: wake(true) wakes all.
    #[test]
    fn wake_all_wakes_all(
        num_waiters in 0u32..30,
    ) {
        let mut f = Futex::init(0);
        for i in 0..num_waiters {
            let mut t = Thread::new(i, Priority::new((i % 31) + 1).unwrap());
            t.dispatch();
            f.wait(0, t);
        }
        prop_assert_eq!(f.num_waiters(), num_waiters);

        let result = f.wake(true);
        prop_assert_eq!(result.woken, num_waiters);
        prop_assert_eq!(f.num_waiters(), 0);
    }

    /// Value is preserved across wait and wake operations.
    #[test]
    fn value_preserved_across_operations(
        val in 0u32..1000,
    ) {
        let mut f = Futex::init(val);

        // Wait (match)
        let mut t1 = Thread::new(1, Priority::new(5).unwrap());
        t1.dispatch();
        f.wait(val, t1);
        prop_assert_eq!(f.val_get(), val);

        // Wake
        f.wake(false);
        prop_assert_eq!(f.val_get(), val);

        // Wait (mismatch)
        let mut t2 = Thread::new(2, Priority::new(5).unwrap());
        t2.dispatch();
        let mismatch_val = if val == 0 { 1 } else { 0 };
        f.wait(mismatch_val, t2);
        prop_assert_eq!(f.val_get(), val);
    }

    /// Wait-wake roundtrip: wait then wake returns to empty queue.
    #[test]
    fn wait_wake_roundtrip(
        val in 0u32..100,
        thread_id in 0u32..1000,
        prio in 1u32..32,
    ) {
        let mut f = Futex::init(val);
        let mut t = Thread::new(thread_id, Priority::new(prio).unwrap());
        t.dispatch();

        f.wait(val, t);
        prop_assert_eq!(f.num_waiters(), 1);

        let result = f.wake(false);
        prop_assert_eq!(result.woken, 1);
        prop_assert_eq!(f.num_waiters(), 0);
    }
}
