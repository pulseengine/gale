//! Plain Rust condition variable for testing and Rocq-of-Rust translation.
//!
//! Identical logic to the Verus-annotated src/condvar.rs.
//! Any divergence between these files is a bug.
//!
//! Source mapping:
//!   z_impl_k_condvar_init      -> CondVar::init          (condvar.c:21-30)
//!   z_impl_k_condvar_signal    -> CondVar::signal         (condvar.c:44-61)
//!   z_impl_k_condvar_broadcast -> CondVar::broadcast      (condvar.c:73-96)
//!   z_impl_k_condvar_wait      -> CondVar::wait_blocking  (condvar.c:99-121)

use crate::error::OK;
use crate::thread::Thread;
use crate::wait_queue::WaitQueue;

/// Result of a signal operation.
#[derive(Debug)]
pub enum SignalResult {
    /// No thread was waiting — signal was a no-op.
    Empty,
    /// The highest-priority waiting thread was woken.
    Woke(Thread),
}

/// Condition variable — port of Zephyr kernel/condvar.c.
#[derive(Debug)]
pub struct CondVar {
    wait_q: WaitQueue,
}

impl CondVar {
    /// z_impl_k_condvar_init (condvar.c:21-30)
    pub fn init() -> Self {
        CondVar {
            wait_q: WaitQueue::new(),
        }
    }

    /// z_impl_k_condvar_signal (condvar.c:44-61)
    pub fn signal(&mut self) -> SignalResult {
        match self.wait_q.unpend_first(OK) {
            Some(t) => SignalResult::Woke(t),
            None => SignalResult::Empty,
        }
    }

    /// z_impl_k_condvar_broadcast (condvar.c:73-96)
    pub fn broadcast(&mut self) -> usize {
        self.wait_q.unpend_all(OK)
    }

    /// z_impl_k_condvar_wait — blocking path (condvar.c:99-121)
    pub fn wait_blocking(&mut self, mut thread: Thread) -> bool {
        thread.block();
        self.wait_q.pend(thread)
    }

    /// Get the number of threads waiting on this condvar.
    pub fn num_waiters(&self) -> usize {
        self.wait_q.len()
    }

    /// Check if any threads are waiting.
    pub fn has_waiters(&self) -> bool {
        self.wait_q.len() > 0
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
    use crate::thread::ThreadState;

    fn make_running_thread(id: u32, prio: u32) -> Thread {
        let mut t = Thread::new(id, Priority::new(prio).unwrap());
        t.dispatch();
        t
    }

    // ---- Init tests ----

    #[test]
    fn test_init() {
        let cv = CondVar::init();
        assert_eq!(cv.num_waiters(), 0);
        assert!(!cv.has_waiters());
    }

    // ---- Signal tests ----

    #[test]
    fn test_signal_empty() {
        let mut cv = CondVar::init();
        assert!(matches!(cv.signal(), SignalResult::Empty));
        assert_eq!(cv.num_waiters(), 0);
    }

    #[test]
    fn test_signal_wakes_one() {
        let mut cv = CondVar::init();
        cv.wait_blocking(make_running_thread(1, 5));
        cv.wait_blocking(make_running_thread(2, 3));
        assert_eq!(cv.num_waiters(), 2);

        // Signal wakes highest priority (thread 2, prio 3)
        match cv.signal() {
            SignalResult::Woke(t) => {
                assert_eq!(t.id, 2);
                assert_eq!(t.state, ThreadState::Ready);
                assert_eq!(t.return_value, OK);
            }
            SignalResult::Empty => panic!("expected Woke"),
        }
        assert_eq!(cv.num_waiters(), 1);
    }

    #[test]
    fn test_signal_wakes_only_one() {
        let mut cv = CondVar::init();
        cv.wait_blocking(make_running_thread(1, 5));
        cv.wait_blocking(make_running_thread(2, 3));
        cv.wait_blocking(make_running_thread(3, 8));

        cv.signal();
        assert_eq!(cv.num_waiters(), 2); // only one removed
    }

    // ---- Broadcast tests ----

    #[test]
    fn test_broadcast_empty() {
        let mut cv = CondVar::init();
        assert_eq!(cv.broadcast(), 0);
        assert_eq!(cv.num_waiters(), 0);
    }

    #[test]
    fn test_broadcast_wakes_all() {
        let mut cv = CondVar::init();
        cv.wait_blocking(make_running_thread(1, 5));
        cv.wait_blocking(make_running_thread(2, 3));
        cv.wait_blocking(make_running_thread(3, 8));
        assert_eq!(cv.num_waiters(), 3);

        let woken = cv.broadcast();
        assert_eq!(woken, 3);
        assert_eq!(cv.num_waiters(), 0);
    }

    #[test]
    fn test_broadcast_returns_count() {
        let mut cv = CondVar::init();
        for i in 0..10 {
            cv.wait_blocking(make_running_thread(i, (i % 32)));
        }
        assert_eq!(cv.broadcast(), 10);
    }

    // ---- Wait tests ----

    #[test]
    fn test_wait_blocking_adds_thread() {
        let mut cv = CondVar::init();
        let result = cv.wait_blocking(make_running_thread(1, 5));
        assert!(result);
        assert_eq!(cv.num_waiters(), 1);
    }

    #[test]
    fn test_wait_blocking_priority_order() {
        let mut cv = CondVar::init();
        cv.wait_blocking(make_running_thread(10, 15));
        cv.wait_blocking(make_running_thread(20, 3)); // highest
        cv.wait_blocking(make_running_thread(30, 8));

        // Signal should wake in priority order
        match cv.signal() {
            SignalResult::Woke(t) => assert_eq!(t.id, 20),
            _ => panic!("expected Woke"),
        }
        match cv.signal() {
            SignalResult::Woke(t) => assert_eq!(t.id, 30),
            _ => panic!("expected Woke"),
        }
        match cv.signal() {
            SignalResult::Woke(t) => assert_eq!(t.id, 10),
            _ => panic!("expected Woke"),
        }
    }

    // ---- Compositional tests ----

    #[test]
    fn test_signal_then_broadcast() {
        let mut cv = CondVar::init();
        cv.wait_blocking(make_running_thread(1, 5));
        cv.wait_blocking(make_running_thread(2, 3));
        cv.wait_blocking(make_running_thread(3, 8));

        // Signal removes one
        cv.signal();
        assert_eq!(cv.num_waiters(), 2);

        // Broadcast removes the rest
        let woken = cv.broadcast();
        assert_eq!(woken, 2);
        assert_eq!(cv.num_waiters(), 0);
    }

    #[test]
    fn test_reuse_after_broadcast() {
        let mut cv = CondVar::init();
        cv.wait_blocking(make_running_thread(1, 5));
        cv.broadcast();
        assert_eq!(cv.num_waiters(), 0);

        // Can reuse after broadcast
        cv.wait_blocking(make_running_thread(2, 3));
        assert_eq!(cv.num_waiters(), 1);
        match cv.signal() {
            SignalResult::Woke(t) => assert_eq!(t.id, 2),
            _ => panic!("expected Woke"),
        }
    }

    #[test]
    fn test_multiple_signal_drains_queue() {
        let mut cv = CondVar::init();
        let n = 5;
        for i in 0..n {
            cv.wait_blocking(make_running_thread(i, (i % 32)));
        }

        for _ in 0..n {
            assert!(matches!(cv.signal(), SignalResult::Woke(_)));
        }
        assert!(matches!(cv.signal(), SignalResult::Empty));
        assert_eq!(cv.num_waiters(), 0);
    }
}
