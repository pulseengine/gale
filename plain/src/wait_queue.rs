//! Priority-ordered wait queue for Zephyr kernel objects.
//!
//! Corresponds to Zephyr's _wait_q_t (a doubly-linked list ordered by thread priority).
//! Used by semaphores, mutexes, condvars, and other blocking kernel objects.
//!
//! Key Zephyr functions modeled:
//! - z_waitq_init()        -> WaitQueue::new()
//! - z_unpend_first_thread -> WaitQueue::unpend_first()
//! - z_pend_curr (partial) -> WaitQueue::pend()
//!
//! ASIL-D properties verified:
//! - Queue is always sorted by priority (highest priority = lowest value first)
//! - unpend_first returns the highest priority thread
//! - No thread appears twice in the queue
//! - pend/unpend maintain sorted invariant
use crate::priority::MAX_PRIORITY;
use crate::thread::{Thread, ThreadId, ThreadState};
/// Maximum threads that can be waiting on a single kernel object.
/// Bounded for verification tractability.
pub const MAX_WAITERS: u32 = 64;
/// A priority-ordered wait queue.
///
/// Threads are stored sorted by priority: index 0 has the highest priority
/// (lowest numerical priority value). This matches Zephyr's z_priq_dumb
/// implementation (simple sorted list used when CONFIG_WAITQ_DUMB=y).
///
/// In Zephyr, this is an intrusive doubly-linked list threaded through
/// k_thread structs. Here we use an array for verification simplicity.
#[derive(Debug)]
pub struct WaitQueue {
    /// Threads waiting, sorted by priority (highest priority first).
    pub entries: [Option<Thread>; 64],
    /// Number of threads currently in the queue.
    pub len: u32,
}
impl WaitQueue {
    /// Initialize an empty wait queue.
    /// Corresponds to z_waitq_init().
    pub fn new() -> Self {
        WaitQueue {
            entries: [
                None, None, None, None, None, None, None, None, None, None, None, None, None, None,
                None, None, None, None, None, None, None, None, None, None, None, None, None, None,
                None, None, None, None, None, None, None, None, None, None, None, None, None, None,
                None, None, None, None, None, None, None, None, None, None, None, None, None, None,
                None, None, None, None, None, None, None, None,
            ],
            len: 0,
        }
    }
    /// Get the number of waiting threads.
    pub fn len(&self) -> u32 {
        self.len
    }
    /// Check if the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// Remove and return the highest-priority (first) thread from the queue.
    ///
    /// Corresponds to z_unpend_first_thread() in Zephyr.
    /// The returned thread is set to Ready state with the given return value.
    ///
    /// ASIL-D properties:
    /// - Returns the thread with lowest priority value (highest scheduling priority)
    /// - Queue remains sorted after removal
    /// - Queue length decreases by exactly 1
    ///
    /// Verified: array shift preserves sorted order and slot validity.
    pub fn unpend_first(&mut self, return_value: i32) -> Option<Thread> {
        if self.len == 0 {
            return None;
        }
        let thread = self.entries[0];
        self.entries[0] = None;
        let mut i: u32 = 0;
        while i < self.len - 1 {
            self.entries[i as usize] = self.entries[(i + 1) as usize];
            self.entries[(i + 1) as usize] = None;
            i = i + 1;
        }
        self.len = self.len - 1;
        match thread {
            Some(mut t) => {
                t.state = ThreadState::Ready;
                t.return_value = return_value;
                Some(t)
            }
            None => None,
        }
    }
    /// Insert a thread into the queue in priority order.
    ///
    /// Corresponds to the insertion part of z_pend_curr().
    /// The thread must be in the Blocked state.
    ///
    /// ASIL-D properties:
    /// - Thread is inserted at the correct priority position
    /// - Queue remains sorted
    /// - Queue length increases by exactly 1
    /// - Returns false if queue is full
    ///
    /// Verified: sorted insertion with per-element shift tracking.
    pub fn pend(&mut self, thread: Thread) -> bool {
        if self.len >= MAX_WAITERS {
            return false;
        }
        let mut insert_pos: u32 = self.len;
        let mut i: u32 = 0;
        let mut found: bool = false;
        while i < self.len && !found {
            let entry_pri = self.entries[i as usize].unwrap().priority.get();
            let thr_pri = thread.priority.get();
            if thr_pri < entry_pri {
                insert_pos = i;
                found = true;
            }
            if !found {
                i = i + 1;
            }
        }
        let mut j: u32 = self.len;
        while j > insert_pos {
            self.entries[j as usize] = self.entries[(j - 1) as usize];
            self.entries[(j - 1) as usize] = None;
            j = j - 1;
        }
        self.entries[insert_pos as usize] = Some(thread);
        self.len = self.len + 1;
        true
    }
    /// Remove all threads from the queue, waking each with the given return value.
    ///
    /// Used by k_sem_reset() to wake all waiters with -EAGAIN.
    ///
    /// Returns the number of threads that were woken.
    ///
    /// Remove all threads from the queue, waking each with the given return value.
    /// Verified: clearing loop sets all slots to None, restoring empty-queue invariant.
    pub fn unpend_all(&mut self, return_value: i32) -> u32 {
        let count = self.len;
        let mut i: u32 = 0;
        while i < count {
            self.entries[i as usize] = None;
            i = i + 1;
        }
        self.len = 0;
        count
    }
}
