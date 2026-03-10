//! Property-based tests for the mutex.
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

use gale::error::*;
use gale::mutex::{LockResult, Mutex};
use gale::priority::Priority;
use gale::thread::Thread;
use proptest::prelude::*;

/// Operations that can be performed on a mutex.
#[derive(Debug, Clone)]
enum MutexOp {
    TryLock { thread_id: u32 },
    Unlock { thread_id: u32 },
    LockBlocking { thread_id: u32, priority: u32 },
    IsLocked,
    LockCountGet,
}

fn mutex_op_strategy() -> impl Strategy<Value = MutexOp> {
    prop_oneof![
        (0u32..10).prop_map(|id| MutexOp::TryLock { thread_id: id }),
        (0u32..10).prop_map(|id| MutexOp::Unlock { thread_id: id }),
        (0u32..1000, 0u32..32).prop_map(|(id, prio)| MutexOp::LockBlocking {
            thread_id: id,
            priority: prio,
        }),
        Just(MutexOp::IsLocked),
        Just(MutexOp::LockCountGet),
    ]
}

proptest! {
    /// M1: lock_count > 0 ⟺ owner.is_some() holds after any sequence.
    #[test]
    fn m1_invariant_holds_under_random_ops(
        ops in prop::collection::vec(mutex_op_strategy(), 0..200),
    ) {
        let mut m = Mutex::init();

        for op in ops {
            match op {
                MutexOp::TryLock { thread_id } => {
                    m.try_lock(thread_id);
                }
                MutexOp::Unlock { thread_id } => {
                    let _ = m.unlock(thread_id);
                }
                MutexOp::LockBlocking { thread_id, priority } => {
                    // Only block if mutex is locked by someone else
                    if m.is_locked() && m.owner_get() != Some(thread_id) && m.num_waiters() < 60 {
                        if let Some(p) = Priority::new(priority) {
                            let mut t = Thread::new(thread_id, p);
                            t.dispatch();
                            m.lock_blocking(t);
                        }
                    }
                }
                MutexOp::IsLocked => { m.is_locked(); }
                MutexOp::LockCountGet => { m.lock_count_get(); }
            }

            // INVARIANT M1 CHECK
            let lc = m.lock_count_get();
            let owner = m.owner_get();
            prop_assert!(
                (lc > 0) == owner.is_some(),
                "M1 violation: lock_count={lc}, owner={owner:?}"
            );

            // INVARIANT M2 CHECK: waiters => locked
            if m.num_waiters() > 0 {
                prop_assert!(m.is_locked(), "M2 violation: waiters but not locked");
            }
        }
    }

    /// Lock-unlock roundtrip: lock then unlock returns to unlocked.
    #[test]
    fn lock_unlock_roundtrip(thread_id in 0u32..1000) {
        let mut m = Mutex::init();
        prop_assert_eq!(m.try_lock(thread_id), LockResult::Acquired);
        prop_assert!(m.is_locked());
        m.unlock(thread_id).unwrap();
        prop_assert!(!m.is_locked());
        prop_assert_eq!(m.lock_count_get(), 0);
        prop_assert_eq!(m.owner_get(), None);
    }

    /// Reentrant lock-unlock: n locks require n unlocks.
    #[test]
    fn reentrant_depth_matches(
        thread_id in 0u32..1000,
        depth in 1u32..50,
    ) {
        let mut m = Mutex::init();
        for _ in 0..depth {
            prop_assert_eq!(m.try_lock(thread_id), LockResult::Acquired);
        }
        prop_assert_eq!(m.lock_count_get(), depth);

        for remaining in (1..depth).rev() {
            m.unlock(thread_id).unwrap();
            prop_assert_eq!(m.lock_count_get(), remaining);
            prop_assert!(m.is_locked());
        }
        m.unlock(thread_id).unwrap();
        prop_assert!(!m.is_locked());
    }

    /// Different thread cannot acquire locked mutex.
    #[test]
    fn contention_preserves_state(
        owner_id in 0u32..500,
        other_id in 500u32..1000,
    ) {
        let mut m = Mutex::init();
        m.try_lock(owner_id);
        prop_assert_eq!(m.try_lock(other_id), LockResult::WouldBlock);
        prop_assert_eq!(m.owner_get(), Some(owner_id));
        prop_assert_eq!(m.lock_count_get(), 1);
    }

    /// Non-owner unlock returns error and preserves state.
    #[test]
    fn non_owner_unlock_error(
        owner_id in 0u32..500,
        other_id in 500u32..1000,
    ) {
        let mut m = Mutex::init();
        m.try_lock(owner_id);
        let result = m.unlock(other_id);
        prop_assert!(matches!(result, Err(EPERM)));
        prop_assert_eq!(m.owner_get(), Some(owner_id));
        prop_assert_eq!(m.lock_count_get(), 1);
    }

    /// Unlock of unlocked mutex returns EINVAL.
    #[test]
    fn unlock_unlocked_error(thread_id in 0u32..1000) {
        let mut m = Mutex::init();
        let result = m.unlock(thread_id);
        prop_assert!(matches!(result, Err(EINVAL)));
    }
}
