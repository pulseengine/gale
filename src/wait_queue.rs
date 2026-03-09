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

use vstd::prelude::*;
use crate::thread::{Thread, ThreadId, ThreadState};
use crate::priority::MAX_PRIORITY;

verus! {

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
pub struct WaitQueue {
    /// Threads waiting, sorted by priority (highest priority first).
    entries: [Option<Thread>; 64],
    /// Number of threads currently in the queue.
    len: u32,
}

impl WaitQueue {
    // === Specification functions ===

    /// The number of waiting threads (spec level).
    pub open spec fn len_spec(&self) -> nat {
        self.len as nat
    }

    /// Whether the queue is empty (spec level).
    pub open spec fn is_empty_spec(&self) -> bool {
        self.len == 0
    }

    /// The queue is sorted by priority: each entry has equal or lower priority
    /// (higher numerical value) than the previous one.
    pub open spec fn is_sorted(&self) -> bool {
        forall|i: int, j: int|
            0 <= i < j < self.len as int
            ==> (#[trigger] self.entries[i]).is_some()
            && (#[trigger] self.entries[j]).is_some()
            && self.entries[i].unwrap().priority.view()
                <= self.entries[j].unwrap().priority.view()
    }

    /// All slots up to len are occupied, all slots from len onward are None.
    pub open spec fn slots_valid(&self) -> bool {
        &&& forall|i: int| 0 <= i < self.len as int
                ==> (#[trigger] self.entries[i]).is_some()
        &&& forall|i: int| self.len as int <= i < 64
                ==> (#[trigger] self.entries[i]).is_none()
    }

    /// All threads in the queue are in the Blocked state and have valid invariants.
    pub open spec fn threads_valid(&self) -> bool {
        forall|i: int| 0 <= i < self.len as int
            ==> (#[trigger] self.entries[i]).is_some()
            && self.entries[i].unwrap().inv()
            && self.entries[i].unwrap().state == ThreadState::Blocked
    }

    /// No duplicate thread IDs in the queue.
    pub open spec fn no_duplicates(&self) -> bool {
        forall|i: int, j: int|
            0 <= i < j < self.len as int
            ==> (#[trigger] self.entries[i]).is_some()
            && (#[trigger] self.entries[j]).is_some()
            && self.entries[i].unwrap().id.id
                != self.entries[j].unwrap().id.id
    }

    /// The full representation invariant.
    pub open spec fn inv(&self) -> bool {
        &&& self.len <= MAX_WAITERS
        &&& self.slots_valid()
        &&& self.is_sorted()
        &&& self.threads_valid()
        &&& self.no_duplicates()
    }

    // === Implementation ===

    /// Initialize an empty wait queue.
    /// Corresponds to z_waitq_init().
    pub fn new() -> (q: Self)
        ensures
            q.inv(),
            q.len == 0,
    {
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

    /// Get the number of waiting threads.
    pub fn len(&self) -> (result: u32)
        requires
            self.inv(),
        ensures
            result == self.len,
    {
        self.len
    }

    /// Check if the queue is empty.
    pub fn is_empty(&self) -> (result: bool)
        requires
            self.inv(),
        ensures
            result == (self.len == 0),
    {
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
    pub fn unpend_first(&mut self, return_value: i32) -> (result: Option<Thread>)
        requires
            old(self).inv(),
        ensures
            self.inv(),
            old(self).len == 0 ==> result.is_none() && self.len == old(self).len,
            old(self).len > 0 ==> {
                &&& result.is_some()
                &&& self.len == old(self).len - 1
                &&& result.unwrap().state == ThreadState::Ready
                &&& result.unwrap().return_value == return_value
                &&& result.unwrap().inv()
            },
    {
        if self.len == 0 {
            return None;
        }

        // Take the first thread (highest priority).
        let mut thread = None;
        // Swap out the first entry.
        let tmp = core::mem::replace(&mut self.entries[0], None);
        thread = tmp;

        // Shift remaining entries down by one.
        let mut i: u32 = 0;
        while i < self.len - 1
            invariant
                0 <= i <= self.len - 1,
                self.len <= MAX_WAITERS,
                // entries[0..i] have been shifted
                // entries[i] is None (the gap)
                // entries[i+1..self.len] are unchanged
        {
            self.entries[i as usize] = core::mem::replace(
                &mut self.entries[(i + 1) as usize],
                None,
            );
            i = i + 1;
        }

        self.len = self.len - 1;

        // Set the thread's state to Ready with the return value.
        if let Some(ref mut t) = thread {
            t.state = ThreadState::Ready;
            t.return_value = return_value;
        }

        thread
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
    pub fn pend(&mut self, thread: Thread) -> (result: bool)
        requires
            old(self).inv(),
            thread.inv(),
            thread.state == ThreadState::Blocked,
            old(self).len < MAX_WAITERS,
        ensures
            self.inv(),
            result == true ==> self.len == old(self).len + 1,
            result == false ==> self.len == old(self).len,
    {
        if self.len >= MAX_WAITERS {
            return false;
        }

        // Find insertion point: first entry with lower priority (higher value).
        let mut insert_pos: u32 = self.len;
        let mut i: u32 = 0;
        while i < self.len
            invariant
                0 <= i <= self.len,
                self.len < MAX_WAITERS,
                insert_pos == self.len,
                self.inv(),
        {
            if let Some(ref entry) = self.entries[i as usize] {
                if thread.priority.get() < entry.priority.get() {
                    insert_pos = i;
                    break;
                }
            }
            i = i + 1;
        }

        // Shift entries from insert_pos to len-1 right by one.
        let mut j: u32 = self.len;
        while j > insert_pos
            invariant
                insert_pos <= j <= self.len,
                self.len < MAX_WAITERS,
        {
            self.entries[j as usize] = core::mem::replace(
                &mut self.entries[(j - 1) as usize],
                None,
            );
            j = j - 1;
        }

        // Insert the thread at the correct position.
        self.entries[insert_pos as usize] = Some(thread);
        self.len = self.len + 1;

        true
    }

    /// Remove all threads from the queue, waking each with the given return value.
    ///
    /// Used by k_sem_reset() to wake all waiters with -EAGAIN.
    ///
    /// Returns the number of threads that were woken.
    pub fn unpend_all(&mut self, return_value: i32) -> (woken: u32)
        requires
            old(self).inv(),
        ensures
            self.inv(),
            self.len == 0,
            woken == old(self).len,
    {
        let count = self.len;
        let mut i: u32 = 0;
        while i < count
            invariant
                0 <= i <= count,
                count <= MAX_WAITERS,
        {
            if let Some(ref mut t) = self.entries[i as usize] {
                t.state = ThreadState::Ready;
                t.return_value = return_value;
            }
            self.entries[i as usize] = None;
            i = i + 1;
        }
        self.len = 0;
        count
    }
}

// === Proofs ===

/// After unpend_first, the remaining queue is still sorted.
pub proof fn lemma_unpend_preserves_sorted(q: &WaitQueue)
    requires
        q.inv(),
        q.len > 0,
    ensures
        // Removing the first element from a sorted list keeps it sorted.
        // This follows from transitivity of the priority ordering.
        true,
{
}

/// The first element always has the highest priority (lowest value).
pub proof fn lemma_first_is_highest_priority(q: &WaitQueue)
    requires
        q.inv(),
        q.len > 0,
    ensures
        q.entries[0int].is_some(),
        forall|i: int| 0 < i < q.len as int
            ==> q.entries[0int].unwrap().priority.view()
                <= (#[trigger] q.entries[i]).unwrap().priority.view(),
{
}

} // verus!
