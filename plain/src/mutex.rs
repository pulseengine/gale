//! Plain Rust mutex for testing and Rocq-of-Rust translation.
//!
//! Identical logic to the Verus-annotated src/mutex.rs.
//! Any divergence between these files is a bug.
//!
//! Source mapping:
//!   z_impl_k_mutex_init   -> Mutex::init         (mutex.c:55-71)
//!   z_impl_k_mutex_lock   -> Mutex::try_lock      (mutex.c:107-154)
//!                          -> Mutex::lock_blocking (mutex.c:169)
//!   z_impl_k_mutex_unlock -> Mutex::unlock        (mutex.c:230-307)

use crate::error::{EINVAL, EPERM, OK};
#[cfg(test)]
use crate::thread::ThreadState;
use crate::thread::Thread;
use crate::wait_queue::WaitQueue;

/// Result of a lock attempt.
#[derive(Debug, PartialEq, Eq)]
pub enum LockResult {
    /// Lock acquired (first time or reentrant).
    Acquired,
    /// Mutex locked by another thread, caller chose not to wait.
    WouldBlock,
}

/// Result of an unlock operation.
#[derive(Debug)]
pub enum UnlockResult {
    /// lock_count decremented, mutex still held by current owner.
    Released,
    /// Fully unlocked, no waiters were present.
    Unlocked,
    /// Ownership transferred to highest-priority waiter.
    Transferred(Thread),
}

/// Reentrant mutex with ownership tracking — port of Zephyr kernel/mutex.c.
#[derive(Debug)]
pub struct Mutex {
    wait_q: WaitQueue,
    owner: Option<u32>,
    lock_count: u32,
}

impl Mutex {
    /// z_impl_k_mutex_init (mutex.c:55-71)
    pub fn init() -> Self {
        Mutex {
            wait_q: WaitQueue::new(),
            owner: None,
            lock_count: 0,
        }
    }

    /// z_impl_k_mutex_lock — non-blocking fast path (mutex.c:107-154)
    pub fn try_lock(&mut self, current_id: u32) -> LockResult {
        if self.lock_count == 0 {
            // Mutex unlocked — acquire.
            self.owner = Some(current_id);
            self.lock_count = 1;
            LockResult::Acquired
        } else if self.owner == Some(current_id) {
            // Reentrant lock — same owner.
            self.lock_count = self.lock_count.checked_add(1).unwrap_or(self.lock_count);
            LockResult::Acquired
        } else {
            // Different owner — cannot acquire.
            LockResult::WouldBlock
        }
    }

    /// z_impl_k_mutex_lock — blocking path (mutex.c:169).
    /// Returns true if thread was enqueued, false if queue is full.
    pub fn lock_blocking(&mut self, mut thread: Thread) -> bool {
        thread.block();
        self.wait_q.pend(thread)
    }

    /// z_impl_k_mutex_unlock (mutex.c:230-307)
    pub fn unlock(&mut self, current_id: u32) -> Result<UnlockResult, i32> {
        // CHECKIF(mutex->owner == NULL)
        if self.owner.is_none() {
            return Err(EINVAL);
        }

        // CHECKIF(mutex->owner != _current)
        if self.owner != Some(current_id) {
            return Err(EPERM);
        }

        // lock_count > 1: reentrant release
        if self.lock_count > 1 {
            self.lock_count = self.lock_count.saturating_sub(1);
            return Ok(UnlockResult::Released);
        }

        // lock_count == 1: final unlock
        if let Some(t) = self.wait_q.unpend_first(OK) {
            // Transfer ownership to highest-priority waiter.
            self.owner = Some(t.id);
            // lock_count stays at 1 (Zephyr doesn't touch it here).
            Ok(UnlockResult::Transferred(t))
        } else {
            // No waiters — fully unlock.
            self.owner = None;
            self.lock_count = 0;
            Ok(UnlockResult::Unlocked)
        }
    }

    /// Check if the mutex is locked.
    pub fn is_locked(&self) -> bool {
        self.lock_count > 0
    }

    /// Get the current lock count.
    pub fn lock_count_get(&self) -> u32 {
        self.lock_count
    }

    /// Get the owner thread ID, if locked.
    pub fn owner_get(&self) -> Option<u32> {
        self.owner
    }

    /// Get the number of threads waiting on this mutex.
    pub fn num_waiters(&self) -> usize {
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
    fn test_init() {
        let m = Mutex::init();
        assert!(!m.is_locked());
        assert_eq!(m.lock_count_get(), 0);
        assert_eq!(m.owner_get(), None);
        assert_eq!(m.num_waiters(), 0);
    }

    // ---- Lock tests ----

    #[test]
    fn test_lock_unlocked() {
        let mut m = Mutex::init();
        assert_eq!(m.try_lock(1), LockResult::Acquired);
        assert!(m.is_locked());
        assert_eq!(m.lock_count_get(), 1);
        assert_eq!(m.owner_get(), Some(1));
    }

    #[test]
    fn test_lock_reentrant() {
        let mut m = Mutex::init();
        assert_eq!(m.try_lock(1), LockResult::Acquired);
        assert_eq!(m.try_lock(1), LockResult::Acquired);
        assert_eq!(m.try_lock(1), LockResult::Acquired);
        assert_eq!(m.lock_count_get(), 3);
        assert_eq!(m.owner_get(), Some(1));
    }

    #[test]
    fn test_lock_different_owner() {
        let mut m = Mutex::init();
        assert_eq!(m.try_lock(1), LockResult::Acquired);
        assert_eq!(m.try_lock(2), LockResult::WouldBlock);
        assert_eq!(m.lock_count_get(), 1);
        assert_eq!(m.owner_get(), Some(1));
    }

    #[test]
    fn test_lock_blocking() {
        let mut m = Mutex::init();
        assert_eq!(m.try_lock(1), LockResult::Acquired);

        let t = make_running_thread(2, 5);
        assert!(m.lock_blocking(t));
        assert_eq!(m.num_waiters(), 1);
        assert_eq!(m.owner_get(), Some(1));
    }

    // ---- Unlock tests ----

    #[test]
    fn test_unlock_not_locked() {
        let mut m = Mutex::init();
        assert!(matches!(m.unlock(1), Err(EINVAL)));
    }

    #[test]
    fn test_unlock_not_owner() {
        let mut m = Mutex::init();
        m.try_lock(1);
        assert!(matches!(m.unlock(2), Err(EPERM)));
        assert_eq!(m.lock_count_get(), 1);
    }

    #[test]
    fn test_unlock_reentrant() {
        let mut m = Mutex::init();
        m.try_lock(1);
        m.try_lock(1);
        m.try_lock(1);
        assert_eq!(m.lock_count_get(), 3);

        assert!(matches!(m.unlock(1), Ok(UnlockResult::Released)));
        assert_eq!(m.lock_count_get(), 2);
        assert_eq!(m.owner_get(), Some(1));

        assert!(matches!(m.unlock(1), Ok(UnlockResult::Released)));
        assert_eq!(m.lock_count_get(), 1);
    }

    #[test]
    fn test_unlock_final_no_waiters() {
        let mut m = Mutex::init();
        m.try_lock(1);
        assert!(matches!(m.unlock(1), Ok(UnlockResult::Unlocked)));
        assert!(!m.is_locked());
        assert_eq!(m.lock_count_get(), 0);
        assert_eq!(m.owner_get(), None);
    }

    #[test]
    fn test_unlock_transfers_to_waiter() {
        let mut m = Mutex::init();
        m.try_lock(1);

        let t = make_running_thread(2, 5);
        m.lock_blocking(t);

        match m.unlock(1) {
            Ok(UnlockResult::Transferred(woken)) => {
                assert_eq!(woken.id, 2);
                assert_eq!(woken.state, ThreadState::Ready);
                assert_eq!(woken.return_value, OK);
            }
            other => panic!("expected Transferred, got {:?}", other),
        }
        assert_eq!(m.owner_get(), Some(2));
        assert_eq!(m.lock_count_get(), 1);
        assert_eq!(m.num_waiters(), 0);
    }

    #[test]
    fn test_unlock_transfers_highest_priority() {
        let mut m = Mutex::init();
        m.try_lock(1);

        m.lock_blocking(make_running_thread(10, 15));
        m.lock_blocking(make_running_thread(20, 3)); // highest priority
        m.lock_blocking(make_running_thread(30, 8));
        assert_eq!(m.num_waiters(), 3);

        // First transfer: highest priority (thread 20, prio 3)
        match m.unlock(1) {
            Ok(UnlockResult::Transferred(woken)) => {
                assert_eq!(woken.id, 20);
            }
            other => panic!("expected Transferred, got {:?}", other),
        }
        assert_eq!(m.owner_get(), Some(20));
        assert_eq!(m.num_waiters(), 2);

        // Second transfer: next priority (thread 30, prio 8)
        match m.unlock(20) {
            Ok(UnlockResult::Transferred(woken)) => {
                assert_eq!(woken.id, 30);
            }
            other => panic!("expected Transferred, got {:?}", other),
        }
        assert_eq!(m.owner_get(), Some(30));
        assert_eq!(m.num_waiters(), 1);
    }

    // ---- Compositional tests ----

    #[test]
    fn test_lock_unlock_roundtrip() {
        let mut m = Mutex::init();
        assert!(!m.is_locked());

        m.try_lock(1);
        assert!(m.is_locked());

        m.unlock(1).unwrap();
        assert!(!m.is_locked());
        assert_eq!(m.lock_count_get(), 0);
        assert_eq!(m.owner_get(), None);
    }

    #[test]
    fn test_reentrant_full_unwind() {
        let mut m = Mutex::init();
        for _ in 0..10 {
            m.try_lock(1);
        }
        assert_eq!(m.lock_count_get(), 10);

        for i in (1..10).rev() {
            assert!(matches!(m.unlock(1), Ok(UnlockResult::Released)));
            assert_eq!(m.lock_count_get(), i);
        }
        assert!(matches!(m.unlock(1), Ok(UnlockResult::Unlocked)));
        assert!(!m.is_locked());
    }

    #[test]
    fn test_reacquire_after_full_unlock() {
        let mut m = Mutex::init();
        m.try_lock(1);
        m.unlock(1).unwrap();

        // Different thread can now acquire
        assert_eq!(m.try_lock(2), LockResult::Acquired);
        assert_eq!(m.owner_get(), Some(2));
    }
}
