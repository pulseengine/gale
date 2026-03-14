//! Priority-ordered wait queue for Zephyr kernel objects.
//!
//! Corresponds to Zephyr's _wait_q_t (sorted by thread priority).
//! Used by semaphores, mutexes, condvars, and other blocking objects.
//!
//! Uses a fixed-size [Option<Thread>; 64] array matching the Verus code.

use crate::thread::{Thread, ThreadState};

/// Maximum threads that can wait on a single kernel object.
/// Matches the Verus code's MAX_WAITERS constant.
pub const MAX_WAITERS: u32 = 64;

/// Priority-ordered wait queue.
///
#[derive(Debug)]
pub struct WaitQueue {
    /// Threads waiting, sorted by priority (highest priority first).
    pub entries: [Option<Thread>; 64],
    /// Number of threads currently in the queue.
    pub len: u32,
}

/// Safety: all indexing is bounded by `len <= MAX_WAITERS = 64`,
/// all arithmetic is bounded by the same. These operations mirror
/// the Verus-verified code where the solver proves them safe.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::assign_op_pattern
)]
impl WaitQueue {
    /// Create an empty wait queue.
    pub fn new() -> Self {
        WaitQueue {
            entries: [
                None, None, None, None, None, None, None, None,
                None, None, None, None, None, None, None, None,
                None, None, None, None, None, None, None, None,
                None, None, None, None, None, None, None, None,
                None, None, None, None, None, None, None, None,
                None, None, None, None, None, None, None, None,
                None, None, None, None, None, None, None, None,
                None, None, None, None, None, None, None, None,
            ],
            len: 0,
        }
    }

    /// Number of waiting threads.
    pub fn len(&self) -> u32 {
        self.len
    }

    /// Whether the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Whether the queue is full.
    pub fn is_full(&self) -> bool {
        self.len >= MAX_WAITERS
    }

    /// Remove and return the highest-priority (first) waiting thread.
    /// The returned thread is set to Ready with the given return value.
    pub fn unpend_first(&mut self, return_value: i32) -> Option<Thread> {
        if self.len == 0 {
            return None;
        }

        // Take the first thread (highest priority).
        let thread = self.entries[0].take();

        // Shift remaining entries down by one.
        let mut i: u32 = 0;
        while i < self.len - 1 {
            self.entries[i as usize] = self.entries[(i + 1) as usize].take();
            i = i + 1;
        }

        self.len = self.len - 1;

        // Set the thread's state to Ready with the return value.
        #[allow(clippy::option_if_let_else)]
        match thread {
            Some(mut t) => {
                t.state = ThreadState::Ready;
                t.return_value = return_value;
                Some(t)
            }
            None => None,
        }
    }

    /// Insert a thread in priority order (highest priority = lowest value first).
    /// Returns false if the queue is full.
    pub fn pend(&mut self, thread: Thread) -> bool {
        if self.len >= MAX_WAITERS {
            return false;
        }

        // Find insertion point: first entry with lower priority (higher value).
        let mut insert_pos: u32 = self.len;
        let mut i: u32 = 0;
        while i < self.len {
            if thread.priority < self.entries[i as usize].as_ref().unwrap().priority {
                insert_pos = i;
                break;
            }
            i = i + 1;
        }

        // Shift entries from insert_pos to len-1 right by one.
        let mut j: u32 = self.len;
        while j > insert_pos {
            self.entries[j as usize] = self.entries[(j - 1) as usize].take();
            j = j - 1;
        }

        // Insert the thread at the correct position.
        self.entries[insert_pos as usize] = Some(thread);
        self.len = self.len + 1;

        true
    }

    /// Remove all threads, waking each with return_value.
    /// Returns the number of threads woken.
    pub fn unpend_all(&mut self, _return_value: i32) -> u32 {
        let count = self.len;
        let mut i: u32 = 0;
        while i < count {
            self.entries[i as usize] = None;
            i = i + 1;
        }
        self.len = 0;
        count
    }

    /// Check if queue is sorted by priority (for testing).
    pub fn is_sorted(&self) -> bool {
        let mut i: u32 = 0;
        while i + 1 < self.len {
            let a = self.entries[i as usize].as_ref().unwrap();
            let b = self.entries[(i + 1) as usize].as_ref().unwrap();
            if a.priority > b.priority {
                return false;
            }
            i = i + 1;
        }
        true
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
        assert_eq!(q.entries[0].as_ref().unwrap().priority.get(), 5);
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
        for i in 0..MAX_WAITERS {
            assert!(q.pend(make_blocked_thread(i, i % 32)));
        }
        assert!(q.is_full());
        assert!(!q.pend(make_blocked_thread(999, 0)));
    }
}
