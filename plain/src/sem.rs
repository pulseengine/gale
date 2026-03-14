//! Plain Rust semaphore for testing and Rocq-of-Rust translation.
//!
//! Identical logic to the Verus-annotated src/sem.rs.
//! Any divergence between these files is a bug.
//!
//! Source mapping:
//!   z_impl_k_sem_init  -> Semaphore::init     (sem.c:45-73)
//!   z_impl_k_sem_give  -> Semaphore::give      (sem.c:95-121)
//!   z_impl_k_sem_take  -> Semaphore::try_take   (sem.c:132-164)
//!   z_impl_k_sem_reset -> Semaphore::reset     (sem.c:166-192)
//!   k_sem_count_get    -> Semaphore::count_get (kernel.h inline)

use crate::error::{EAGAIN, EINVAL, OK};
use crate::thread::Thread;
use crate::wait_queue::WaitQueue;

/// Result of a give operation.
#[derive(Debug)]
pub enum GiveResult {
    /// Count was incremented (no waiters present).
    Incremented,
    /// A waiting thread was woken (count unchanged).
    WokeThread(Thread),
    /// Count was already at limit (no-op).
    Saturated,
}

/// Result of a take operation.
#[derive(Debug, PartialEq, Eq)]
pub enum TakeResult {
    /// Semaphore was available; count decremented.
    Acquired,
    /// Semaphore unavailable, caller chose not to wait.
    WouldBlock,
    /// Semaphore unavailable, caller is now blocked on the wait queue.
    Blocked,
}

/// Counting semaphore — port of Zephyr kernel/sem.c.
#[derive(Debug)]
pub struct Semaphore {
    pub wait_q: WaitQueue,
    pub count: u32,
    pub limit: u32,
}

impl Semaphore {
    /// z_impl_k_sem_init (sem.c:45-73)
    pub fn init(initial_count: u32, limit: u32) -> Result<Self, i32> {
        if limit == 0 || initial_count > limit {
            return Err(EINVAL);
        }
        Ok(Semaphore {
            wait_q: WaitQueue::new(),
            count: initial_count,
            limit,
        })
    }

    /// z_impl_k_sem_give (sem.c:95-121)
    #[allow(clippy::arithmetic_side_effects)]
    pub fn give(&mut self) -> GiveResult {
        if let Some(thread) = self.wait_q.unpend_first(OK) {
            GiveResult::WokeThread(thread)
        } else if self.count != self.limit {
            self.count = self.count + 1;
            GiveResult::Incremented
        } else {
            GiveResult::Saturated
        }
    }

    /// z_impl_k_sem_take — non-blocking (sem.c:132-164 with K_NO_WAIT)
    #[allow(clippy::arithmetic_side_effects)]
    pub fn try_take(&mut self) -> TakeResult {
        if self.count > 0 {
            self.count = self.count - 1;
            TakeResult::Acquired
        } else {
            TakeResult::WouldBlock
        }
    }

    /// z_impl_k_sem_take — blocking path.
    /// Returns true if acquired immediately, false if thread was blocked.
    /// Returns false without blocking if the wait queue is full.
    pub fn take_blocking(&mut self, mut thread: Thread) -> bool {
        thread.block();
        self.wait_q.pend(thread);
        false
    }

    /// z_impl_k_sem_reset (sem.c:166-192)
    pub fn reset(&mut self) -> u32 {
        let woken = self.wait_q.unpend_all(EAGAIN);
        self.count = 0;
        woken
    }

    /// k_sem_count_get (kernel.h inline)
    pub fn count_get(&self) -> u32 {
        self.count
    }

    pub fn limit_get(&self) -> u32 {
        self.limit
    }

    pub fn num_waiters(&self) -> u32 {
        self.wait_q.len()
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::wildcard_enum_match_arm,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
mod tests {
    use super::*;
    use crate::priority::Priority;

    fn make_running_thread(id: u32, prio: u32) -> Thread {
        let mut t = Thread::new(id, Priority::new(prio).unwrap());
        t.dispatch();
        t
    }

    // ---- Init tests ----

    #[test]
    fn test_init_valid() {
        let sem = Semaphore::init(0, 10).unwrap();
        assert_eq!(sem.count_get(), 0);
        assert_eq!(sem.limit_get(), 10);
    }

    #[test]
    fn test_init_at_limit() {
        let sem = Semaphore::init(5, 5).unwrap();
        assert_eq!(sem.count_get(), 5);
    }

    #[test]
    fn test_init_rejects_zero_limit() {
        assert!(matches!(Semaphore::init(0, 0), Err(EINVAL)));
    }

    #[test]
    fn test_init_rejects_count_over_limit() {
        assert!(matches!(Semaphore::init(11, 10), Err(EINVAL)));
    }

    // ---- Give tests ----

    #[test]
    fn test_give_increments() {
        let mut sem = Semaphore::init(0, 5).unwrap();
        match sem.give() {
            GiveResult::Incremented => {}
            _ => panic!("expected Incremented"),
        }
        assert_eq!(sem.count_get(), 1);
    }

    #[test]
    fn test_give_saturates_at_limit() {
        let mut sem = Semaphore::init(5, 5).unwrap();
        match sem.give() {
            GiveResult::Saturated => {}
            _ => panic!("expected Saturated"),
        }
        assert_eq!(sem.count_get(), 5);
    }

    #[test]
    fn test_give_wakes_waiter() {
        let mut sem = Semaphore::init(0, 5).unwrap();
        let t = make_running_thread(1, 5);
        sem.take_blocking(t);
        assert_eq!(sem.num_waiters(), 1);

        match sem.give() {
            GiveResult::WokeThread(woken) => {
                assert_eq!(woken.id.id, 1);
                assert_eq!(woken.state, crate::thread::ThreadState::Ready);
                assert_eq!(woken.return_value, OK);
            }
            _ => panic!("expected WokeThread"),
        }
        assert_eq!(sem.count_get(), 0);
    }

    // ---- Take tests ----

    #[test]
    fn test_try_take_available() {
        let mut sem = Semaphore::init(3, 5).unwrap();
        assert_eq!(sem.try_take(), TakeResult::Acquired);
        assert_eq!(sem.count_get(), 2);
    }

    #[test]
    fn test_try_take_unavailable() {
        let mut sem = Semaphore::init(0, 5).unwrap();
        assert_eq!(sem.try_take(), TakeResult::WouldBlock);
        assert_eq!(sem.count_get(), 0);
    }

    #[test]
    fn test_take_blocking_blocks_thread() {
        let mut sem = Semaphore::init(0, 5).unwrap();
        let t = make_running_thread(1, 5);
        let acquired = sem.take_blocking(t);
        assert!(!acquired);
        assert_eq!(sem.num_waiters(), 1);
    }

    // ---- Reset tests ----

    #[test]
    fn test_reset_clears_count() {
        let mut sem = Semaphore::init(3, 5).unwrap();
        sem.reset();
        assert_eq!(sem.count_get(), 0);
    }

    #[test]
    fn test_reset_wakes_all_waiters() {
        let mut sem = Semaphore::init(0, 5).unwrap();
        sem.take_blocking(make_running_thread(1, 5));
        sem.take_blocking(make_running_thread(2, 3));
        sem.take_blocking(make_running_thread(3, 7));
        assert_eq!(sem.num_waiters(), 3);

        let woken = sem.reset();
        assert_eq!(woken, 3);
        assert_eq!(sem.num_waiters(), 0);
        assert_eq!(sem.count_get(), 0);
    }

    // ---- Compositional tests ----

    #[test]
    fn test_give_take_roundtrip() {
        let mut sem = Semaphore::init(3, 10).unwrap();
        sem.give();
        assert_eq!(sem.count_get(), 4);
        sem.try_take();
        assert_eq!(sem.count_get(), 3);
    }

    #[test]
    fn test_invariant_preserved_through_operations() {
        let mut sem = Semaphore::init(0, 5).unwrap();
        // Invariant: 0 <= count <= limit
        for _ in 0..100 {
            sem.give();
            assert!(sem.count_get() <= sem.limit_get());
        }
        // Should have saturated at 5
        assert_eq!(sem.count_get(), 5);

        for _ in 0..100 {
            sem.try_take();
            assert!(sem.count_get() <= sem.limit_get());
        }
        // Should be at 0
        assert_eq!(sem.count_get(), 0);
    }

    #[test]
    fn test_give_wakes_highest_priority_first() {
        let mut sem = Semaphore::init(0, 5).unwrap();
        sem.take_blocking(make_running_thread(1, 10));
        sem.take_blocking(make_running_thread(2, 3)); // highest priority
        sem.take_blocking(make_running_thread(3, 7));

        match sem.give() {
            GiveResult::WokeThread(t) => assert_eq!(t.id.id, 2),
            _ => panic!("expected WokeThread"),
        }
        match sem.give() {
            GiveResult::WokeThread(t) => assert_eq!(t.id.id, 3),
            _ => panic!("expected WokeThread"),
        }
        match sem.give() {
            GiveResult::WokeThread(t) => assert_eq!(t.id.id, 1),
            _ => panic!("expected WokeThread"),
        }
    }
}
