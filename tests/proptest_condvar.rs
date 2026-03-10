//! Property-based tests for the condition variable.
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

use gale::condvar::CondVar;
use gale::priority::Priority;
use gale::thread::Thread;
use proptest::prelude::*;

/// Operations that can be performed on a condvar.
#[derive(Debug, Clone)]
enum CondVarOp {
    Signal,
    Broadcast,
    WaitBlocking { thread_id: u32, priority: u32 },
    NumWaiters,
}

fn condvar_op_strategy() -> impl Strategy<Value = CondVarOp> {
    prop_oneof![
        Just(CondVarOp::Signal),
        Just(CondVarOp::Broadcast),
        (0u32..1000, 0u32..32).prop_map(|(id, prio)| CondVarOp::WaitBlocking {
            thread_id: id,
            priority: prio,
        }),
        Just(CondVarOp::NumWaiters),
    ]
}

proptest! {
    /// Wait queue length is always non-negative and bounded.
    #[test]
    fn waiters_bounded_under_random_ops(
        ops in prop::collection::vec(condvar_op_strategy(), 0..200),
    ) {
        let mut cv = CondVar::init();

        for op in ops {
            match op {
                CondVarOp::Signal => { cv.signal(); }
                CondVarOp::Broadcast => { cv.broadcast(); }
                CondVarOp::WaitBlocking { thread_id, priority } => {
                    if cv.num_waiters() < 60 {
                        if let Some(p) = Priority::new(priority) {
                            let mut t = Thread::new(thread_id, p);
                            t.dispatch();
                            cv.wait_blocking(t);
                        }
                    }
                }
                CondVarOp::NumWaiters => { cv.num_waiters(); }
            }

            // Waiters count is always sensible
            prop_assert!(cv.num_waiters() <= 64);
        }
    }

    /// Broadcast always empties the queue.
    #[test]
    fn broadcast_empties_queue(
        num_waiters in 0u32..30,
    ) {
        let mut cv = CondVar::init();
        for i in 0..num_waiters {
            let p = Priority::new(i % 32).unwrap();
            let mut t = Thread::new(i, p);
            t.dispatch();
            cv.wait_blocking(t);
        }

        let woken = cv.broadcast();
        prop_assert_eq!(woken, num_waiters as usize);
        prop_assert_eq!(cv.num_waiters(), 0);
    }

    /// N signals on N waiters empties the queue.
    #[test]
    fn n_signals_empties_n_waiters(
        num_waiters in 0u32..30,
    ) {
        let mut cv = CondVar::init();
        for i in 0..num_waiters {
            let p = Priority::new(i % 32).unwrap();
            let mut t = Thread::new(i, p);
            t.dispatch();
            cv.wait_blocking(t);
        }

        for _ in 0..num_waiters {
            cv.signal();
        }
        prop_assert_eq!(cv.num_waiters(), 0);
    }

    /// Signal on empty is a no-op.
    #[test]
    fn signal_empty_noop(n in 0u32..100) {
        let mut cv = CondVar::init();
        for _ in 0..n {
            cv.signal();
        }
        prop_assert_eq!(cv.num_waiters(), 0);
    }

    /// Broadcast returns woken count matching initial waiter count.
    #[test]
    fn broadcast_returns_correct_count(
        num_waiters in 0u32..30,
        extra_signals in 0u32..10,
    ) {
        let mut cv = CondVar::init();
        for i in 0..num_waiters {
            let p = Priority::new(i % 32).unwrap();
            let mut t = Thread::new(i, p);
            t.dispatch();
            cv.wait_blocking(t);
        }

        // Signal removes some
        let signals = extra_signals.min(num_waiters);
        for _ in 0..signals {
            cv.signal();
        }

        // Broadcast removes the rest
        let remaining = num_waiters - signals;
        let woken = cv.broadcast();
        prop_assert_eq!(woken, remaining as usize);
    }
}
