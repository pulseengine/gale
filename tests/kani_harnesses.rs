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
    use gale::sem::{Semaphore, TakeResult};

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
            assert_eq!(result, TakeResult::Acquired);
            assert_eq!(sem.count_get(), count - 1);
        } else {
            assert_eq!(result, TakeResult::WouldBlock);
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
    use gale::thread::ThreadId;

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

        assert_eq!(m.try_lock(ThreadId { id: thread_id }), LockResult::Acquired);
        assert_eq!(m.owner_get(), Some(ThreadId { id: thread_id }));
        assert_eq!(m.lock_count_get(), 1);
    }

    /// M4: reentrant lock increments count.
    #[kani::proof]
    #[kani::unwind(6)]
    fn mutex_lock_reentrant() {
        let thread_id: u32 = kani::any();
        let mut m = Mutex::init();
        m.try_lock(ThreadId { id: thread_id });

        let depth: u32 = kani::any();
        kani::assume(depth > 0 && depth <= 5);

        for _ in 0..depth {
            assert_eq!(m.try_lock(ThreadId { id: thread_id }), LockResult::Acquired);
        }
        assert_eq!(m.lock_count_get(), depth + 1);
        assert_eq!(m.owner_get(), Some(ThreadId { id: thread_id }));
    }

    /// M5: different thread gets WouldBlock.
    #[kani::proof]
    fn mutex_lock_contended() {
        let owner: u32 = kani::any();
        let other: u32 = kani::any();
        kani::assume(owner != other);

        let mut m = Mutex::init();
        m.try_lock(ThreadId { id: owner });

        assert_eq!(m.try_lock(ThreadId { id: other }), LockResult::WouldBlock);
        assert_eq!(m.owner_get(), Some(ThreadId { id: owner }));
        assert_eq!(m.lock_count_get(), 1);
    }

    /// M6a: unlock with no owner returns EINVAL.
    #[kani::proof]
    fn mutex_unlock_not_locked() {
        let thread_id: u32 = kani::any();
        let mut m = Mutex::init();
        assert!(matches!(m.unlock(ThreadId { id: thread_id }), Err(EINVAL)));
    }

    /// M6b: unlock by non-owner returns EPERM.
    #[kani::proof]
    fn mutex_unlock_not_owner() {
        let owner: u32 = kani::any();
        let other: u32 = kani::any();
        kani::assume(owner != other);

        let mut m = Mutex::init();
        m.try_lock(ThreadId { id: owner });
        assert!(matches!(m.unlock(ThreadId { id: other }), Err(EPERM)));
        assert_eq!(m.lock_count_get(), 1);
    }

    /// M7: reentrant unlock decrements count.
    #[kani::proof]
    fn mutex_unlock_reentrant() {
        let thread_id: u32 = kani::any();
        let mut m = Mutex::init();
        m.try_lock(ThreadId { id: thread_id });
        m.try_lock(ThreadId { id: thread_id });
        m.try_lock(ThreadId { id: thread_id });

        assert!(matches!(
            m.unlock(ThreadId { id: thread_id }),
            Ok(UnlockResult::Released)
        ));
        assert_eq!(m.lock_count_get(), 2);
        assert_eq!(m.owner_get(), Some(ThreadId { id: thread_id }));
    }

    /// M9: final unlock fully releases mutex.
    #[kani::proof]
    fn mutex_unlock_final() {
        let thread_id: u32 = kani::any();
        let mut m = Mutex::init();
        m.try_lock(ThreadId { id: thread_id });

        assert!(matches!(
            m.unlock(ThreadId { id: thread_id }),
            Ok(UnlockResult::Unlocked)
        ));
        assert!(!m.is_locked());
        assert_eq!(m.lock_count_get(), 0);
        assert!(m.owner_get().is_none());
    }

    /// Lock-unlock roundtrip returns to init state.
    #[kani::proof]
    fn mutex_lock_unlock_roundtrip() {
        let thread_id: u32 = kani::any();
        let mut m = Mutex::init();
        m.try_lock(ThreadId { id: thread_id });
        m.unlock(ThreadId { id: thread_id }).unwrap();
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
            m.try_lock(ThreadId { id: thread_id });
            i += 1;
        }
        assert_eq!(m.lock_count_get(), depth);

        i = 0;
        while i < depth {
            m.unlock(ThreadId { id: thread_id }).unwrap();
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
                0 => {
                    m.try_lock(ThreadId { id: tid });
                }
                1 => {
                    let _ = m.unlock(ThreadId { id: tid });
                }
                _ => {
                    m.is_locked();
                }
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
        assert_eq!(woken, n);
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

// ---------------------------------------------------------------------------
// Message queue harnesses
// ---------------------------------------------------------------------------

#[cfg(kani)]
mod kani_msgq_proofs {
    use gale::error::*;
    use gale::msgq::MsgQ;

    /// MQ2: init rejects invalid parameters.
    #[kani::proof]
    fn msgq_init_validates_parameters() {
        let msg_size: u32 = kani::any();
        let max_msgs: u32 = kani::any();

        kani::assume(msg_size <= 256);
        kani::assume(max_msgs <= 64);

        match MsgQ::init(msg_size, max_msgs) {
            Ok(mq) => {
                assert!(msg_size > 0);
                assert!(max_msgs > 0);
                assert!(msg_size.checked_mul(max_msgs).is_some());
                assert_eq!(mq.num_used_get(), 0);
                assert_eq!(mq.num_free_get(), max_msgs);
            }
            Err(e) => {
                assert_eq!(e, EINVAL);
                assert!(msg_size == 0 || max_msgs == 0 || msg_size.checked_mul(max_msgs).is_none());
            }
        }
    }

    /// MQ5/MQ6: put preserves invariant.
    #[kani::proof]
    #[kani::unwind(9)]
    fn msgq_put_preserves_invariant() {
        let max_msgs: u32 = kani::any();
        kani::assume(max_msgs > 0 && max_msgs <= 8);

        let mut mq = MsgQ::init(4, max_msgs).unwrap();

        // Put some messages
        let n: u32 = kani::any();
        kani::assume(n <= max_msgs);

        let mut i: u32 = 0;
        while i < n {
            mq.put().unwrap();
            i += 1;
        }

        // Try one more
        let result = mq.put();
        if n < max_msgs {
            assert!(result.is_ok());
            assert_eq!(mq.num_used_get(), n + 1);
        } else {
            assert!(result.is_err());
            assert_eq!(mq.num_used_get(), n);
        }
        assert!(mq.num_used_get() <= mq.max_msgs_get());
    }

    /// MQ8/MQ9: get preserves invariant.
    #[kani::proof]
    #[kani::unwind(9)]
    fn msgq_get_preserves_invariant() {
        let max_msgs: u32 = kani::any();
        kani::assume(max_msgs > 0 && max_msgs <= 8);

        let mut mq = MsgQ::init(4, max_msgs).unwrap();

        let n: u32 = kani::any();
        kani::assume(n <= max_msgs);

        let mut i: u32 = 0;
        while i < n {
            mq.put().unwrap();
            i += 1;
        }

        let result = mq.get();
        if n > 0 {
            assert!(result.is_ok());
            assert_eq!(mq.num_used_get(), n - 1);
        } else {
            assert!(result.is_err());
            assert_eq!(mq.num_used_get(), 0);
        }
    }

    /// MQ7: put_front retreats read_idx correctly.
    #[kani::proof]
    #[kani::unwind(9)]
    fn msgq_put_front_preserves_invariant() {
        let max_msgs: u32 = kani::any();
        kani::assume(max_msgs > 0 && max_msgs <= 8);

        let mut mq = MsgQ::init(4, max_msgs).unwrap();

        let n: u32 = kani::any();
        kani::assume(n < max_msgs);

        let mut i: u32 = 0;
        while i < n {
            mq.put().unwrap();
            i += 1;
        }

        let result = mq.put_front();
        assert!(result.is_ok());
        assert_eq!(mq.num_used_get(), n + 1);
        assert!(mq.num_used_get() <= mq.max_msgs_get());
    }

    /// MQ10: peek_at returns valid slot.
    #[kani::proof]
    #[kani::unwind(9)]
    fn msgq_peek_at_valid_slot() {
        let max_msgs: u32 = kani::any();
        kani::assume(max_msgs > 0 && max_msgs <= 8);

        let mut mq = MsgQ::init(4, max_msgs).unwrap();

        let n: u32 = kani::any();
        kani::assume(n > 0 && n <= max_msgs);

        let mut i: u32 = 0;
        while i < n {
            mq.put().unwrap();
            i += 1;
        }

        let idx: u32 = kani::any();
        kani::assume(idx < n);
        let slot = mq.peek_at(idx).unwrap();
        assert!(slot < max_msgs);
    }

    /// MQ11: purge resets queue.
    #[kani::proof]
    #[kani::unwind(9)]
    fn msgq_purge_resets() {
        let max_msgs: u32 = kani::any();
        kani::assume(max_msgs > 0 && max_msgs <= 8);

        let mut mq = MsgQ::init(4, max_msgs).unwrap();

        let n: u32 = kani::any();
        kani::assume(n <= max_msgs);

        let mut i: u32 = 0;
        while i < n {
            mq.put().unwrap();
            i += 1;
        }

        let old_used = mq.purge();
        assert_eq!(old_used, n);
        assert_eq!(mq.num_used_get(), 0);
        assert!(mq.is_empty());
        assert_eq!(mq.read_idx_get(), mq.write_idx_get());
    }

    /// MQ13: ring consistency after operation sequence.
    #[kani::proof]
    #[kani::unwind(6)]
    fn msgq_operation_sequence_ring_consistency() {
        let max_msgs: u32 = kani::any();
        kani::assume(max_msgs > 0 && max_msgs <= 4);

        let mut mq = MsgQ::init(4, max_msgs).unwrap();

        for _ in 0..5 {
            let op: u8 = kani::any();
            kani::assume(op < 4);
            match op {
                0 => {
                    let _ = mq.put();
                }
                1 => {
                    let _ = mq.get();
                }
                2 => {
                    let _ = mq.put_front();
                }
                _ => {
                    mq.purge();
                }
            }
            assert!(mq.num_used_get() <= mq.max_msgs_get());
            let expected = (mq.read_idx_get() + mq.num_used_get()) % mq.max_msgs_get();
            assert_eq!(mq.write_idx_get(), expected);
        }
    }

    /// Fill-drain roundtrip returns to empty.
    #[kani::proof]
    #[kani::unwind(6)]
    fn msgq_fill_drain_roundtrip() {
        let max_msgs: u32 = kani::any();
        kani::assume(max_msgs > 0 && max_msgs <= 5);

        let mut mq = MsgQ::init(4, max_msgs).unwrap();

        let mut i: u32 = 0;
        while i < max_msgs {
            mq.put().unwrap();
            i += 1;
        }
        assert!(mq.is_full());

        i = 0;
        while i < max_msgs {
            mq.get().unwrap();
            i += 1;
        }
        assert!(mq.is_empty());
    }
}

// ---------------------------------------------------------------------------
// Stack harnesses
// ---------------------------------------------------------------------------

#[cfg(kani)]
mod kani_stack_proofs {
    use gale::error::*;
    use gale::stack::Stack;

    /// SK1/SK2: init rejects zero capacity, accepts valid.
    #[kani::proof]
    fn stack_init_validates_parameters() {
        let capacity: u32 = kani::any();
        kani::assume(capacity <= 100);

        match Stack::init(capacity) {
            Ok(s) => {
                assert!(capacity > 0);
                assert_eq!(s.num_used(), 0);
                assert_eq!(s.num_free(), capacity);
                assert!(s.is_empty());
            }
            Err(e) => {
                assert_eq!(e, EINVAL);
                assert_eq!(capacity, 0);
            }
        }
    }

    /// SK3/SK4: push preserves invariant.
    #[kani::proof]
    #[kani::unwind(17)]
    fn stack_push_preserves_invariant() {
        let capacity: u32 = kani::any();
        kani::assume(capacity > 0 && capacity <= 16);

        let mut s = Stack::init(capacity).unwrap();

        let n: u32 = kani::any();
        kani::assume(n <= capacity);

        let mut i: u32 = 0;
        while i < n {
            assert_eq!(s.push(), OK);
            i += 1;
        }

        let rc = s.push();
        if n < capacity {
            assert_eq!(rc, OK);
            assert_eq!(s.num_used(), n + 1);
        } else {
            assert_eq!(rc, ENOMEM);
            assert_eq!(s.num_used(), n);
        }
    }

    /// SK5/SK6: pop preserves invariant.
    #[kani::proof]
    #[kani::unwind(17)]
    fn stack_pop_preserves_invariant() {
        let capacity: u32 = kani::any();
        kani::assume(capacity > 0 && capacity <= 16);

        let mut s = Stack::init(capacity).unwrap();

        let n: u32 = kani::any();
        kani::assume(n <= capacity);

        let mut i: u32 = 0;
        while i < n {
            s.push();
            i += 1;
        }

        let rc = s.pop();
        if n > 0 {
            assert_eq!(rc, OK);
            assert_eq!(s.num_used(), n - 1);
        } else {
            assert_eq!(rc, EBUSY);
            assert_eq!(s.num_used(), 0);
        }
    }

    /// SK7: conservation after arbitrary operations.
    #[kani::proof]
    #[kani::unwind(6)]
    fn stack_conservation_after_ops() {
        let capacity: u32 = kani::any();
        kani::assume(capacity > 0 && capacity <= 8);

        let mut s = Stack::init(capacity).unwrap();

        for _ in 0..5 {
            let push: bool = kani::any();
            if push {
                s.push();
            } else {
                s.pop();
            }
            assert_eq!(s.num_free() + s.num_used(), capacity);
        }
    }

    /// SK9: push-pop roundtrip preserves state.
    #[kani::proof]
    #[kani::unwind(17)]
    fn stack_push_pop_roundtrip() {
        let capacity: u32 = kani::any();
        kani::assume(capacity > 0 && capacity <= 16);

        let fill: u32 = kani::any();
        kani::assume(fill < capacity);

        let mut s = Stack::init(capacity).unwrap();
        let mut i: u32 = 0;
        while i < fill {
            s.push();
            i += 1;
        }
        let original = s;

        assert_eq!(s.push(), OK);
        assert_eq!(s.pop(), OK);
        assert_eq!(s, original);
    }

    /// Fill-drain roundtrip returns to empty.
    #[kani::proof]
    #[kani::unwind(6)]
    fn stack_fill_drain_roundtrip() {
        let capacity: u32 = kani::any();
        kani::assume(capacity > 0 && capacity <= 5);

        let mut s = Stack::init(capacity).unwrap();

        let mut i: u32 = 0;
        while i < capacity {
            assert_eq!(s.push(), OK);
            i += 1;
        }
        assert!(s.is_full());

        i = 0;
        while i < capacity {
            assert_eq!(s.pop(), OK);
            i += 1;
        }
        assert!(s.is_empty());
    }
}

// ---------------------------------------------------------------------------
// Pipe harnesses
// ---------------------------------------------------------------------------

#[cfg(kani)]
mod kani_pipe_proofs {
    use gale::error::*;
    use gale::pipe::Pipe;

    /// PP1: init establishes invariant.
    #[kani::proof]
    fn pipe_init_invariant() {
        let size: u32 = kani::any();
        kani::assume(size <= 256);

        match Pipe::init(size) {
            Ok(p) => {
                assert!(size > 0);
                assert!(p.is_empty());
                assert!(p.is_open());
                assert!(!p.is_resetting());
                assert_eq!(p.space_get() + p.data_get(), size);
            }
            Err(e) => {
                assert_eq!(e, EINVAL);
                assert_eq!(size, 0);
            }
        }
    }

    /// PP2/PP8: write preserves invariant and computes correct byte count.
    #[kani::proof]
    fn pipe_write_preserves_invariant() {
        let size: u32 = kani::any();
        kani::assume(size > 0 && size <= 16);

        let fill: u32 = kani::any();
        kani::assume(fill <= size);

        let request: u32 = kani::any();
        kani::assume(request <= 64);

        let mut p = Pipe::init(size).unwrap();
        if fill > 0 {
            p.write_check(fill).unwrap();
        }

        let free = p.space_get();
        match p.write_check(request) {
            Ok(n) => {
                assert!(n > 0);
                assert!(n <= request);
                assert!(n <= free);
                if request <= free {
                    assert_eq!(n, request);
                } else {
                    assert_eq!(n, free);
                }
            }
            Err(EAGAIN) => assert_eq!(free, 0),
            Err(ENOMSG) => assert_eq!(request, 0),
            Err(_) => panic!("unexpected error"),
        }
        assert_eq!(p.space_get() + p.data_get(), size);
    }

    /// PP3/PP9: read preserves invariant and computes correct byte count.
    #[kani::proof]
    fn pipe_read_preserves_invariant() {
        let size: u32 = kani::any();
        kani::assume(size > 0 && size <= 16);

        let fill: u32 = kani::any();
        kani::assume(fill <= size);

        let request: u32 = kani::any();
        kani::assume(request <= 64);

        let mut p = Pipe::init(size).unwrap();
        if fill > 0 {
            p.write_check(fill).unwrap();
        }

        let used = p.data_get();
        match p.read_check(request) {
            Ok(n) => {
                assert!(n > 0);
                assert!(n <= request);
                assert!(n <= used);
                if request <= used {
                    assert_eq!(n, request);
                } else {
                    assert_eq!(n, used);
                }
            }
            Err(EAGAIN) => assert_eq!(used, 0),
            Err(ENOMSG) => assert_eq!(request, 0),
            Err(_) => panic!("unexpected error"),
        }
        assert_eq!(p.space_get() + p.data_get(), size);
    }

    /// PP4/PP5: error codes for state transitions.
    #[kani::proof]
    fn pipe_error_codes_correct() {
        let size: u32 = kani::any();
        kani::assume(size > 0 && size <= 16);

        let mut p = Pipe::init(size).unwrap();

        // Closed pipe
        let mut closed = p;
        closed.close();
        assert_eq!(closed.write_check(1), Err(EPIPE));
        assert_eq!(closed.read_check(1), Err(EPIPE));

        // Resetting pipe
        p.write_check(1).unwrap();
        let mut resetting = p;
        resetting.reset();
        assert_eq!(resetting.write_check(1), Err(ECANCELED));
        assert_eq!(resetting.read_check(1), Err(ECANCELED));
    }

    /// PP6: full pipe rejects write.
    #[kani::proof]
    fn pipe_full_rejects_write() {
        let size: u32 = kani::any();
        kani::assume(size > 0 && size <= 16);

        let mut p = Pipe::init(size).unwrap();
        p.write_check(size).unwrap();
        assert!(p.is_full());
        assert_eq!(p.write_check(1), Err(EAGAIN));
    }

    /// PP7: empty pipe rejects read.
    #[kani::proof]
    fn pipe_empty_rejects_read() {
        let size: u32 = kani::any();
        kani::assume(size > 0 && size <= 16);

        let p = Pipe::init(size).unwrap();
        assert!(p.is_empty());
        let mut p2 = p;
        assert_eq!(p2.read_check(1), Err(EAGAIN));
    }

    /// PP10: conservation after arbitrary operations.
    #[kani::proof]
    #[kani::unwind(6)]
    fn pipe_conservation_after_ops() {
        let size: u32 = kani::any();
        kani::assume(size > 0 && size <= 8);

        let mut p = Pipe::init(size).unwrap();

        for _ in 0..5 {
            let op: u8 = kani::any();
            kani::assume(op < 4);
            match op {
                0 => {
                    let len: u32 = kani::any();
                    kani::assume(len > 0 && len <= 16);
                    let _ = p.write_check(len);
                }
                1 => {
                    let len: u32 = kani::any();
                    kani::assume(len > 0 && len <= 16);
                    let _ = p.read_check(len);
                }
                2 => p.reset(),
                _ => p.close(),
            }
            assert_eq!(p.space_get() + p.data_get(), size);
        }
    }
}

// ---------------------------------------------------------------------------
// Timer harnesses
// ---------------------------------------------------------------------------

#[cfg(kani)]
mod kani_timer_proofs {
    use gale::error::*;
    use gale::timer::Timer;

    /// TM1/TM6/TM7: init establishes invariant and period classification.
    #[kani::proof]
    fn timer_init_invariant() {
        let period: u32 = kani::any();
        kani::assume(period <= 1000);

        let t = Timer::init(period);
        assert_eq!(t.status_peek(), 0);
        assert_eq!(t.period_get(), period);
        assert!(!t.is_running());
    }

    /// TM3: start sets status = 0 and running = true.
    #[kani::proof]
    #[kani::unwind(11)]
    fn timer_start() {
        let period: u32 = kani::any();
        kani::assume(period <= 1000);

        let mut t = Timer::init(period);

        // Accumulate some status first
        let n: u32 = kani::any();
        kani::assume(n <= 10);
        let mut i: u32 = 0;
        while i < n {
            let _ = t.expire();
            i += 1;
        }

        t.start();
        assert_eq!(t.status_peek(), 0);
        assert!(t.is_running());
        assert_eq!(t.period_get(), period);
    }

    /// TM5: expire increments status by 1.
    #[kani::proof]
    fn timer_expire() {
        let period: u32 = kani::any();
        kani::assume(period <= 1000);

        let status: u32 = kani::any();
        kani::assume(status < u32::MAX);

        let mut t = Timer::init(period);
        t.status = status;

        let result = t.expire();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), status + 1);
        assert_eq!(t.status_peek(), status + 1);
    }

    /// TM2: status_get returns old value and resets to 0.
    #[kani::proof]
    #[kani::unwind(11)]
    fn timer_status_get() {
        let period: u32 = kani::any();
        kani::assume(period <= 1000);

        let mut t = Timer::init(period);

        let n: u32 = kani::any();
        kani::assume(n <= 10);
        let mut i: u32 = 0;
        while i < n {
            let _ = t.expire();
            i += 1;
        }

        let got = t.status_get();
        assert_eq!(got, n);
        assert_eq!(t.status_peek(), 0);
    }

    /// TM8: overflow at u32::MAX returns EOVERFLOW, state unchanged.
    #[kani::proof]
    fn timer_overflow() {
        let period: u32 = kani::any();
        kani::assume(period <= 1000);

        let mut t = Timer::init(period);
        t.status = u32::MAX;

        let result = t.expire();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), EOVERFLOW);
        assert_eq!(t.status_peek(), u32::MAX);
    }
}

// ---------------------------------------------------------------------------
// Event harnesses
// ---------------------------------------------------------------------------

#[cfg(kani)]
mod kani_event_proofs {
    use gale::event::Event;

    /// EV7: init creates event with all bits cleared.
    #[kani::proof]
    fn event_init() {
        let ev = Event::init();
        assert_eq!(ev.events_get(), 0);
    }

    /// EV8: post is monotonic — never clears bits.
    #[kani::proof]
    fn event_post_monotonic() {
        let initial: u32 = kani::any();
        let new_bits: u32 = kani::any();

        let mut ev = Event::init();
        ev.set(initial);
        let before = ev.events_get();

        ev.post(new_bits);
        let after = ev.events_get();

        // All bits set before are still set
        assert_eq!(before & after, before);
        // Result is OR of both
        assert_eq!(after, initial | new_bits);
    }

    /// EV2+EV3: set then clear roundtrip yields 0.
    #[kani::proof]
    fn event_set_clear() {
        let value: u32 = kani::any();

        let mut ev = Event::init();
        ev.set(value);
        assert_eq!(ev.events_get(), value);

        ev.clear(value);
        assert_eq!(ev.events_get(), 0);
    }

    /// EV5: wait_check_any returns true when any desired bit is set.
    #[kani::proof]
    fn event_wait_any() {
        let events: u32 = kani::any();
        let desired: u32 = kani::any();

        let mut ev = Event::init();
        ev.set(events);

        let result = ev.wait_check_any(desired);
        assert_eq!(result, (events & desired) != 0);
    }

    /// EV6: wait_check_all returns true when all desired bits are set.
    #[kani::proof]
    fn event_wait_all() {
        let events: u32 = kani::any();
        let desired: u32 = kani::any();

        let mut ev = Event::init();
        ev.set(events);

        let result = ev.wait_check_all(desired);
        assert_eq!(result, (events & desired) == desired);
    }
}

// ---------------------------------------------------------------------------
// Memory slab harnesses
// ---------------------------------------------------------------------------

#[cfg(kani)]
mod kani_mem_slab_proofs {
    use gale::error::*;
    use gale::mem_slab::MemSlab;

    /// MS1/MS2/MS3: init validates parameters and establishes invariant.
    #[kani::proof]
    fn mem_slab_init() {
        let block_size: u32 = kani::any();
        let num_blocks: u32 = kani::any();

        kani::assume(block_size <= 256);
        kani::assume(num_blocks <= 64);

        match MemSlab::init(block_size, num_blocks) {
            Ok(s) => {
                assert!(block_size > 0);
                assert!(num_blocks > 0);
                assert_eq!(s.num_used_get(), 0);
                assert_eq!(s.num_free_get(), num_blocks);
                assert!(s.is_empty());
            }
            Err(e) => {
                assert_eq!(e, EINVAL);
                assert!(block_size == 0 || num_blocks == 0);
            }
        }
    }

    /// MS4: alloc when not full increments num_used.
    #[kani::proof]
    #[kani::unwind(17)]
    fn mem_slab_alloc() {
        let num_blocks: u32 = kani::any();
        kani::assume(num_blocks > 0 && num_blocks <= 16);

        let mut s = MemSlab::init(4, num_blocks).unwrap();

        let n: u32 = kani::any();
        kani::assume(n <= num_blocks);

        let mut i: u32 = 0;
        while i < n {
            assert_eq!(s.alloc(), OK);
            i += 1;
        }

        let rc = s.alloc();
        if n < num_blocks {
            assert_eq!(rc, OK);
            assert_eq!(s.num_used_get(), n + 1);
        } else {
            assert_eq!(rc, ENOMEM);
            assert_eq!(s.num_used_get(), n);
        }
    }

    /// MS6: free when num_used > 0 decrements num_used.
    #[kani::proof]
    #[kani::unwind(17)]
    fn mem_slab_free() {
        let num_blocks: u32 = kani::any();
        kani::assume(num_blocks > 0 && num_blocks <= 16);

        let mut s = MemSlab::init(4, num_blocks).unwrap();

        let n: u32 = kani::any();
        kani::assume(n <= num_blocks);

        let mut i: u32 = 0;
        while i < n {
            s.alloc();
            i += 1;
        }

        let rc = s.free();
        if n > 0 {
            assert_eq!(rc, OK);
            assert_eq!(s.num_used_get(), n - 1);
        } else {
            assert_eq!(rc, EINVAL);
            assert_eq!(s.num_used_get(), 0);
        }
    }

    /// MS7: conservation after arbitrary operations.
    #[kani::proof]
    #[kani::unwind(6)]
    fn mem_slab_conservation() {
        let num_blocks: u32 = kani::any();
        kani::assume(num_blocks > 0 && num_blocks <= 8);

        let mut s = MemSlab::init(4, num_blocks).unwrap();

        for _ in 0..5 {
            let do_alloc: bool = kani::any();
            if do_alloc {
                s.alloc();
            } else {
                s.free();
            }
            assert_eq!(s.num_free_get() + s.num_used_get(), num_blocks);
        }
    }

    /// MS4+MS6: alloc-free roundtrip preserves state.
    #[kani::proof]
    #[kani::unwind(17)]
    fn mem_slab_roundtrip() {
        let num_blocks: u32 = kani::any();
        kani::assume(num_blocks > 0 && num_blocks <= 16);

        let fill: u32 = kani::any();
        kani::assume(fill < num_blocks);

        let mut s = MemSlab::init(4, num_blocks).unwrap();
        let mut i: u32 = 0;
        while i < fill {
            s.alloc();
            i += 1;
        }
        let original = s;

        assert_eq!(s.alloc(), OK);
        assert_eq!(s.free(), OK);
        assert_eq!(s, original);
    }
}
