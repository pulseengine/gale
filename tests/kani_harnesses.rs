//! Kani model-checking harnesses for the semaphore.
//!
//! Run with: cargo kani --harness <name>
//! These are bounded model checks that exhaustively verify properties
//! within the specified bounds.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::wildcard_enum_match_arm,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

#[cfg(kani)]
mod kani_proofs {
    use gale::error::*;
    use gale::priority::Priority;
    use gale::sem::Semaphore;
    use gale::thread::Thread;

    /// Exhaustively verify init rejects invalid parameters.
    #[kani::proof]
    fn init_validates_parameters() {
        let count: u32 = kani::any();
        let limit: u32 = kani::any();

        // Bound the search space
        kani::assume(count <= 20);
        kani::assume(limit <= 20);

        match Semaphore::init(count, limit) {
            Ok(sem) => {
                assert!(limit > 0);
                assert!(count <= limit);
                assert!(sem.count_get() <= sem.limit_get());
            }
            Err(e) => {
                assert_eq!(e, EINVAL);
                assert!(limit == 0 || count > limit);
            }
        }
    }

    /// Verify give preserves invariant for all reachable states.
    #[kani::proof]
    fn give_preserves_invariant() {
        let count: u32 = kani::any();
        let limit: u32 = kani::any();

        kani::assume(limit > 0 && limit <= 10);
        kani::assume(count <= limit);

        let mut sem = Semaphore::init(count, limit).unwrap();
        sem.give();

        assert!(sem.count_get() <= sem.limit_get());
        assert!(sem.limit_get() == limit);
    }

    /// Verify try_take preserves invariant for all reachable states.
    #[kani::proof]
    fn try_take_preserves_invariant() {
        let count: u32 = kani::any();
        let limit: u32 = kani::any();

        kani::assume(limit > 0 && limit <= 10);
        kani::assume(count <= limit);

        let mut sem = Semaphore::init(count, limit).unwrap();
        let result = sem.try_take();

        assert!(sem.count_get() <= sem.limit_get());
        if count > 0 {
            assert_eq!(result, OK);
            assert_eq!(sem.count_get(), count - 1);
        } else {
            assert_eq!(result, EBUSY);
            assert_eq!(sem.count_get(), 0);
        }
    }

    /// Verify reset brings semaphore to clean state.
    #[kani::proof]
    fn reset_clears_count() {
        let count: u32 = kani::any();
        let limit: u32 = kani::any();

        kani::assume(limit > 0 && limit <= 10);
        kani::assume(count <= limit);

        let mut sem = Semaphore::init(count, limit).unwrap();
        sem.reset();

        assert_eq!(sem.count_get(), 0);
        assert_eq!(sem.limit_get(), limit);
    }

    /// Verify give-take roundtrip returns to original count.
    #[kani::proof]
    fn give_take_roundtrip() {
        let count: u32 = kani::any();
        let limit: u32 = kani::any();

        kani::assume(limit > 0 && limit <= 10);
        kani::assume(count < limit);

        let mut sem = Semaphore::init(count, limit).unwrap();
        sem.give();
        sem.try_take();

        assert_eq!(sem.count_get(), count);
    }

    /// Verify a sequence of N operations maintains the invariant.
    #[kani::proof]
    #[kani::unwind(6)]
    fn operation_sequence_maintains_invariant() {
        let limit: u32 = kani::any();
        kani::assume(limit > 0 && limit <= 5);

        let count: u32 = kani::any();
        kani::assume(count <= limit);

        let mut sem = Semaphore::init(count, limit).unwrap();

        // 5 arbitrary operations
        for _ in 0..5 {
            let op: u8 = kani::any();
            kani::assume(op < 4);
            match op {
                0 => {
                    sem.give();
                }
                1 => {
                    sem.try_take();
                }
                2 => {
                    sem.reset();
                }
                _ => {
                    sem.count_get();
                }
            }
            assert!(sem.count_get() <= sem.limit_get());
        }
    }
}

// ---------------------------------------------------------------------------
// Mutex harnesses
// ---------------------------------------------------------------------------

#[cfg(kani)]
mod kani_mutex_proofs {
    use gale::error::*;
    use gale::mutex::{LockResult, Mutex, UnlockResult};

    /// M1: invariant holds after init.
    #[kani::proof]
    fn mutex_init_invariant() {
        let m = Mutex::init();
        assert!(!m.is_locked());
        assert_eq!(m.lock_count_get(), 0);
        assert!(m.owner_get().is_none());
    }

    /// M3: lock on unlocked mutex acquires.
    #[kani::proof]
    fn mutex_lock_unlocked() {
        let thread_id: u32 = kani::any();
        let mut m = Mutex::init();

        assert_eq!(m.try_lock(thread_id), LockResult::Acquired);
        assert_eq!(m.owner_get(), Some(thread_id));
        assert_eq!(m.lock_count_get(), 1);
    }

    /// M4: reentrant lock increments count.
    #[kani::proof]
    fn mutex_lock_reentrant() {
        let thread_id: u32 = kani::any();
        let mut m = Mutex::init();
        m.try_lock(thread_id);

        let depth: u32 = kani::any();
        kani::assume(depth > 0 && depth <= 5);

        for _ in 0..depth {
            assert_eq!(m.try_lock(thread_id), LockResult::Acquired);
        }
        assert_eq!(m.lock_count_get(), depth + 1);
        assert_eq!(m.owner_get(), Some(thread_id));
    }

    /// M5: different thread gets WouldBlock.
    #[kani::proof]
    fn mutex_lock_contended() {
        let owner: u32 = kani::any();
        let other: u32 = kani::any();
        kani::assume(owner != other);

        let mut m = Mutex::init();
        m.try_lock(owner);

        assert_eq!(m.try_lock(other), LockResult::WouldBlock);
        assert_eq!(m.owner_get(), Some(owner));
        assert_eq!(m.lock_count_get(), 1);
    }

    /// M6a: unlock with no owner returns EINVAL.
    #[kani::proof]
    fn mutex_unlock_not_locked() {
        let thread_id: u32 = kani::any();
        let mut m = Mutex::init();
        assert!(matches!(m.unlock(thread_id), Err(EINVAL)));
    }

    /// M6b: unlock by non-owner returns EPERM.
    #[kani::proof]
    fn mutex_unlock_not_owner() {
        let owner: u32 = kani::any();
        let other: u32 = kani::any();
        kani::assume(owner != other);

        let mut m = Mutex::init();
        m.try_lock(owner);
        assert!(matches!(m.unlock(other), Err(EPERM)));
        assert_eq!(m.lock_count_get(), 1);
    }

    /// M7: reentrant unlock decrements count.
    #[kani::proof]
    fn mutex_unlock_reentrant() {
        let thread_id: u32 = kani::any();
        let mut m = Mutex::init();
        m.try_lock(thread_id);
        m.try_lock(thread_id);
        m.try_lock(thread_id);

        assert!(matches!(m.unlock(thread_id), Ok(UnlockResult::Released)));
        assert_eq!(m.lock_count_get(), 2);
        assert_eq!(m.owner_get(), Some(thread_id));
    }

    /// M9: final unlock fully releases mutex.
    #[kani::proof]
    fn mutex_unlock_final() {
        let thread_id: u32 = kani::any();
        let mut m = Mutex::init();
        m.try_lock(thread_id);

        assert!(matches!(m.unlock(thread_id), Ok(UnlockResult::Unlocked)));
        assert!(!m.is_locked());
        assert_eq!(m.lock_count_get(), 0);
        assert!(m.owner_get().is_none());
    }

    /// Lock-unlock roundtrip returns to init state.
    #[kani::proof]
    fn mutex_lock_unlock_roundtrip() {
        let thread_id: u32 = kani::any();
        let mut m = Mutex::init();
        m.try_lock(thread_id);
        m.unlock(thread_id).unwrap();
        assert!(!m.is_locked());
        assert_eq!(m.lock_count_get(), 0);
    }

    /// Reentrant full unwind: N locks followed by N unlocks.
    #[kani::proof]
    #[kani::unwind(6)]
    fn mutex_reentrant_full_unwind() {
        let thread_id: u32 = kani::any();
        let depth: u32 = kani::any();
        kani::assume(depth > 0 && depth <= 5);

        let mut m = Mutex::init();
        let mut i: u32 = 0;
        while i < depth {
            m.try_lock(thread_id);
            i += 1;
        }
        assert_eq!(m.lock_count_get(), depth);

        i = 0;
        while i < depth {
            m.unlock(thread_id).unwrap();
            i += 1;
        }
        assert!(!m.is_locked());
    }

    /// M1 invariant holds after arbitrary operation sequence.
    #[kani::proof]
    #[kani::unwind(6)]
    fn mutex_operation_sequence_m1() {
        let mut m = Mutex::init();

        for _ in 0..5 {
            let op: u8 = kani::any();
            let tid: u32 = kani::any();
            kani::assume(op < 3);

            match op {
                0 => { m.try_lock(tid); }
                1 => { let _ = m.unlock(tid); }
                _ => { m.is_locked(); }
            }

            // M1 check
            assert!((m.lock_count_get() > 0) == m.owner_get().is_some());
        }
    }
}

// ---------------------------------------------------------------------------
// Condition variable harnesses
// ---------------------------------------------------------------------------

#[cfg(kani)]
mod kani_condvar_proofs {
    use gale::condvar::{CondVar, SignalResult};
    use gale::priority::Priority;
    use gale::thread::Thread;

    fn make_thread(id: u32, prio: u32) -> Thread {
        let p = Priority::new(prio % 32).unwrap();
        let mut t = Thread::new(id, p);
        t.dispatch();
        t
    }

    /// C1: init creates empty condvar.
    #[kani::proof]
    fn condvar_init_empty() {
        let cv = CondVar::init();
        assert_eq!(cv.num_waiters(), 0);
        assert!(!cv.has_waiters());
    }

    /// C3: signal on empty is a no-op.
    #[kani::proof]
    fn condvar_signal_empty() {
        let mut cv = CondVar::init();
        assert!(matches!(cv.signal(), SignalResult::Empty));
        assert_eq!(cv.num_waiters(), 0);
    }

    /// C5: broadcast on empty returns 0.
    #[kani::proof]
    fn condvar_broadcast_empty() {
        let mut cv = CondVar::init();
        assert_eq!(cv.broadcast(), 0);
        assert_eq!(cv.num_waiters(), 0);
    }

    /// C2: signal wakes exactly one.
    #[kani::proof]
    fn condvar_signal_wakes_one() {
        let id1: u32 = kani::any();
        let id2: u32 = kani::any();
        kani::assume(id1 != id2);

        let mut cv = CondVar::init();
        cv.wait_blocking(make_thread(id1, 5));
        cv.wait_blocking(make_thread(id2, 3));
        assert_eq!(cv.num_waiters(), 2);

        cv.signal();
        assert_eq!(cv.num_waiters(), 1);
    }

    /// C4: broadcast wakes all and returns correct count.
    #[kani::proof]
    #[kani::unwind(5)]
    fn condvar_broadcast_wakes_all() {
        let mut cv = CondVar::init();
        let n: u32 = kani::any();
        kani::assume(n <= 4);

        let mut i: u32 = 0;
        while i < n {
            cv.wait_blocking(make_thread(i, i % 32));
            i += 1;
        }
        assert_eq!(cv.num_waiters() as u32, n);

        let woken = cv.broadcast();
        assert_eq!(woken, n as usize);
        assert_eq!(cv.num_waiters(), 0);
    }

    /// C6: wait_blocking adds thread.
    #[kani::proof]
    fn condvar_wait_adds_thread() {
        let id: u32 = kani::any();
        let prio: u32 = kani::any();
        kani::assume(prio < 32);

        let mut cv = CondVar::init();
        let result = cv.wait_blocking(make_thread(id, prio));
        assert!(result);
        assert_eq!(cv.num_waiters(), 1);
    }

    /// Broadcast idempotence.
    #[kani::proof]
    fn condvar_broadcast_idempotent() {
        let mut cv = CondVar::init();
        assert_eq!(cv.broadcast(), 0);
        assert_eq!(cv.broadcast(), 0);
        assert_eq!(cv.num_waiters(), 0);
    }

    /// Signal-broadcast equivalence: N signals then broadcast == broadcast.
    #[kani::proof]
    fn condvar_signal_broadcast_equivalence() {
        let id1: u32 = kani::any();
        let id2: u32 = kani::any();
        kani::assume(id1 != id2);

        let mut cv = CondVar::init();
        cv.wait_blocking(make_thread(id1, 5));
        cv.wait_blocking(make_thread(id2, 3));

        // Signal one, broadcast rest
        cv.signal();
        let woken = cv.broadcast();
        assert_eq!(woken, 1);
        assert_eq!(cv.num_waiters(), 0);
    }
}
