//! Property-based tests for the semaphore.
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

use gale::priority::Priority;
use gale::sem::Semaphore;
use gale::thread::Thread;
use proptest::prelude::*;

/// Operations that can be performed on a semaphore.
#[derive(Debug, Clone)]
enum SemOp {
    Give,
    TryTake,
    TakeBlocking { thread_id: u32, priority: u32 },
    Reset,
    CountGet,
}

fn sem_op_strategy() -> impl Strategy<Value = SemOp> {
    prop_oneof![
        Just(SemOp::Give),
        Just(SemOp::TryTake),
        (0u32..1000, 0u32..32).prop_map(|(id, prio)| SemOp::TakeBlocking {
            thread_id: id,
            priority: prio,
        }),
        Just(SemOp::Reset),
        Just(SemOp::CountGet),
    ]
}

proptest! {
    /// The fundamental invariant (0 <= count <= limit) holds after any
    /// sequence of operations.
    #[test]
    fn invariant_holds_under_random_ops(
        initial_count in 0u32..100,
        limit in 1u32..100,
        ops in prop::collection::vec(sem_op_strategy(), 0..200),
    ) {
        // Clamp initial_count to limit
        let initial_count = initial_count.min(limit);
        let mut sem = Semaphore::init(initial_count, limit).unwrap();

        for op in ops {
            match op {
                SemOp::Give => { sem.give(); }
                SemOp::TryTake => { sem.try_take(); }
                SemOp::TakeBlocking { thread_id, priority } => {
                    if sem.count_get() == 0 {
                        let p = Priority::new(priority).unwrap();
                        let mut t = Thread::new(thread_id, p);
                        t.dispatch();
                        sem.take_blocking(t);
                    }
                }
                SemOp::Reset => { sem.reset(); }
                SemOp::CountGet => { sem.count_get(); }
            }

            // INVARIANT CHECK
            prop_assert!(sem.count_get() <= sem.limit_get());
            prop_assert!(sem.limit_get() == limit);
        }
    }

    /// Give-take roundtrip: give then take returns count to original.
    #[test]
    fn give_take_roundtrip(
        initial_count in 0u32..99,
        limit in 1u32..100,
    ) {
        let initial_count = initial_count.min(limit.saturating_sub(1));
        let limit = limit.max(initial_count + 1);
        let mut sem = Semaphore::init(initial_count, limit).unwrap();
        let before = sem.count_get();
        sem.give();
        sem.try_take();
        prop_assert_eq!(sem.count_get(), before);
    }

    /// Repeated gives saturate at exactly limit.
    #[test]
    fn gives_saturate_at_limit(
        limit in 1u32..1000,
        extra_gives in 0u32..100,
    ) {
        let mut sem = Semaphore::init(0, limit).unwrap();
        for _ in 0..(u64::from(limit) + u64::from(extra_gives)) {
            sem.give();
        }
        prop_assert_eq!(sem.count_get(), limit);
    }

    /// Repeated takes bottom out at 0.
    #[test]
    fn takes_bottom_at_zero(
        initial_count in 0u32..100,
        limit in 1u32..100,
        extra_takes in 0u32..100,
    ) {
        let initial_count = initial_count.min(limit);
        let mut sem = Semaphore::init(initial_count, limit).unwrap();
        for _ in 0..(u64::from(initial_count) + u64::from(extra_takes)) {
            sem.try_take();
        }
        prop_assert_eq!(sem.count_get(), 0);
    }
}
