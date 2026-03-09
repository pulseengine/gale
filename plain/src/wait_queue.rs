//! Priority-ordered wait queue for Zephyr kernel objects.
//!
//! Corresponds to Zephyr's _wait_q_t (sorted by thread priority).
//! Used by semaphores, mutexes, condvars, and other blocking objects.
//!
//! Uses heapless::Vec for no_std bounded allocation — matches the
//! fixed-size [Option<Thread>; 64] array in the Verus code.

use crate::thread::{Thread, ThreadState};
use heapless::Vec;

/// Maximum threads that can wait on a single kernel object.
/// Matches the Verus code's MAX_WAITERS constant.
pub const MAX_WAITERS: usize = 64;

/// Priority-ordered wait queue.
#[derive(Debug)]
pub struct WaitQueue {
    entries: Vec<Thread, MAX_WAITERS>,
}

impl WaitQueue {
    /// Create an empty wait queue.
    pub fn new() -> Self {
        WaitQueue {
            entries: Vec::new(),
        }
    }

    /// Number of waiting threads.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Whether the queue is full.
    pub fn is_full(&self) -> bool {
        self.entries.is_full()
    }

    /// Remove and return the highest-priority (first) waiting thread.
    /// The returned thread is set to Ready with the given return value.
    pub fn unpend_first(&mut self, return_value: i32) -> Option<Thread> {
        if self.entries.is_empty() {
            return None;
        }
        let mut thread = self.entries.remove(0);
        thread.state = ThreadState::Ready;
        thread.return_value = return_value;
        Some(thread)
    }

    /// Insert a thread in priority order (highest priority = lowest value first).
    /// Returns false if the queue is full.
    pub fn pend(&mut self, thread: Thread) -> bool {
        if self.entries.is_full() {
            return false;
        }
        let pos = self
            .entries
            .iter()
            .position(|e| thread.priority < e.priority)
            .unwrap_or(self.entries.len());
        // insert shifts elements right — safe because we checked !is_full() above
        let _ = self.entries.insert(pos, thread);
        true
    }

    /// Remove all threads, waking each with return_value.
    /// Returns the number of threads woken.
    pub fn unpend_all(&mut self, return_value: i32) -> usize {
        let count = self.entries.len();
        for thread in &mut self.entries {
            thread.state = ThreadState::Ready;
            thread.return_value = return_value;
        }
        self.entries.clear();
        count
    }

    /// Check if queue is sorted by priority (for testing).
    pub fn is_sorted(&self) -> bool {
        // windows(2) guarantees each slice has exactly 2 elements
        #[allow(clippy::indexing_slicing)]
        self.entries
            .windows(2)
            .all(|w| w[0].priority <= w[1].priority)
    }

    /// Get a reference to all entries (for testing).
    pub fn entries(&self) -> &[Thread] {
        &self.entries
    }
}

impl Default for WaitQueue {
    fn default() -> Self {
        Self::new()
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

    fn make_blocked_thread(id: u32, prio: u32) -> Thread {
        let mut t = Thread::new(id, Priority::new(prio).unwrap());
        t.dispatch();
        t.block();
        t
    }

    #[test]
    fn test_empty_queue() {
        let mut q = WaitQueue::new();
        assert!(q.is_empty());
        assert_eq!(q.len(), 0);
        assert!(q.unpend_first(0).is_none());
    }

    #[test]
    fn test_pend_maintains_priority_order() {
        let mut q = WaitQueue::new();
        assert!(q.pend(make_blocked_thread(1, 10)));
        assert!(q.pend(make_blocked_thread(2, 5)));
        assert!(q.pend(make_blocked_thread(3, 15)));
        assert!(q.pend(make_blocked_thread(4, 5)));

        assert!(q.is_sorted());
        assert_eq!(q.len(), 4);
        assert_eq!(q.entries()[0].priority.get(), 5);
    }

    #[test]
    fn test_unpend_first_returns_highest_priority() {
        let mut q = WaitQueue::new();
        assert!(q.pend(make_blocked_thread(1, 10)));
        assert!(q.pend(make_blocked_thread(2, 3)));
        assert!(q.pend(make_blocked_thread(3, 7)));

        let t = q.unpend_first(42).unwrap();
        assert_eq!(t.priority.get(), 3);
        assert_eq!(t.state, ThreadState::Ready);
        assert_eq!(t.return_value, 42);
        assert_eq!(q.len(), 2);
    }

    #[test]
    fn test_unpend_all() {
        let mut q = WaitQueue::new();
        assert!(q.pend(make_blocked_thread(1, 5)));
        assert!(q.pend(make_blocked_thread(2, 10)));
        assert!(q.pend(make_blocked_thread(3, 15)));

        let woken = q.unpend_all(-11);
        assert_eq!(woken, 3);
        assert!(q.is_empty());
    }

    #[test]
    fn test_pend_full_returns_false() {
        let mut q = WaitQueue::new();
        for i in 0..MAX_WAITERS as u32 {
            assert!(q.pend(make_blocked_thread(i, i % 32)));
        }
        assert!(q.is_full());
        assert!(!q.pend(make_blocked_thread(999, 0)));
    }
}
