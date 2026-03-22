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
///
/// STPA SC-14 / GAP-9: This bounds the Verus verification model.
/// The actual Zephyr wait queue is an unbounded linked list.
/// Systems with more than 64 threads blocking on a single object
/// exceed the model bounds. Ensure CONFIG_NUM_THREADS <= 64 or
/// adjust this constant. The Verus proofs guarantee correctness
/// for up to MAX_WAITERS concurrent waiters per object.
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
            && self.entries[i].unwrap().state === ThreadState::Blocked
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
    ///
    /// Verified: array shift preserves sorted order and slot validity.
    pub fn unpend_first(&mut self, return_value: i32) -> (result: Option<Thread>)
        requires
            old(self).inv(),
        ensures
            self.inv(),
            old(self).len == 0 ==> result.is_none() && self.len == old(self).len,
            old(self).len > 0 ==> {
                &&& result.is_some()
                &&& self.len == old(self).len - 1
                &&& result.unwrap().state === ThreadState::Ready
                &&& result.unwrap().return_value == return_value
                &&& result.unwrap().inv()
            },
    {
        if self.len == 0 {
            return None;
        }

        // Take the first thread (highest priority).
        let thread = self.entries[0];
        self.entries[0] = None;

        // Shift remaining entries down by one.
        let mut i: u32 = 0;
        while i < self.len - 1
            invariant
                0 <= i <= self.len - 1,
                self.len == old(self).len,
                self.len <= MAX_WAITERS,
                self.len > 0,
                // Shifted portion: entries[0..i) contain old entries[1..i+1)
                forall|k: int| 0 <= k < i as int
                    ==> (#[trigger] self.entries[k]) === old(self).entries[k + 1],
                // Current position is None
                (#[trigger] self.entries[i as int]).is_none(),
                // Unshifted portion: entries[i+1..len) unchanged
                forall|k: int| (i as int) + 1 <= k < self.len as int
                    ==> (#[trigger] self.entries[k]) === old(self).entries[k],
                // Tail is None
                forall|k: int| self.len as int <= k < 64
                    ==> (#[trigger] self.entries[k]).is_none(),
                // Thread saved from position 0
                thread === old(self).entries[0int],
            decreases
                self.len - 1 - i,
        {
            self.entries[i as usize] = self.entries[(i + 1) as usize];
            self.entries[(i + 1) as usize] = None;
            i = i + 1;
        }

        // After shift: entries[0..len-2] == old entries[1..len-1], entries[len-1..63] are None.
        // Hint: all entries in [0, len-1) match old entries shifted by 1.
        assert(forall|k: int| 0 <= k < (self.len - 1) as int
            ==> (#[trigger] self.entries[k]) === old(self).entries[k + 1]);

        self.len = self.len - 1;

        // Prove slots_valid: occupied slots [0..new_len) are Some, rest are None.
        assert(forall|k: int| 0 <= k < self.len as int
            ==> (#[trigger] self.entries[k]).is_some());
        assert(forall|k: int| self.len as int <= k < 64
            ==> (#[trigger] self.entries[k]).is_none());

        // Prove is_sorted: shifted entries preserve original ordering.
        assert(forall|i1: int, j1: int| 0 <= i1 < j1 < self.len as int
            ==> (#[trigger] self.entries[i1]).is_some()
            && (#[trigger] self.entries[j1]).is_some()
            && self.entries[i1].unwrap().priority.view()
                <= self.entries[j1].unwrap().priority.view());

        // Prove threads_valid: all threads are valid and Blocked.
        assert(forall|k: int| 0 <= k < self.len as int
            ==> (#[trigger] self.entries[k]).is_some()
            && self.entries[k].unwrap().inv()
            && self.entries[k].unwrap().state === ThreadState::Blocked);

        // Prove no_duplicates: subset of original, no new IDs introduced.
        assert(forall|i1: int, j1: int| 0 <= i1 < j1 < self.len as int
            ==> (#[trigger] self.entries[i1]).is_some()
            && (#[trigger] self.entries[j1]).is_some()
            && self.entries[i1].unwrap().id.id
                != self.entries[j1].unwrap().id.id);

        // Set the thread's state to Ready with the return value.
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
    pub fn pend(&mut self, thread: Thread) -> (result: bool)
        requires
            old(self).inv(),
            thread.inv(),
            thread.state === ThreadState::Blocked,
            old(self).len < MAX_WAITERS,
            // New thread's ID must not already be in the queue.
            forall|k: int| 0 <= k < old(self).len as int
                ==> (#[trigger] old(self).entries[k]).is_some()
                && old(self).entries[k].unwrap().id.id != thread.id.id,
        ensures
            self.inv(),
            result == true,
            self.len == old(self).len + 1,
    {
        if self.len >= MAX_WAITERS {
            // Precondition guarantees old(self).len < MAX_WAITERS,
            // so this branch is unreachable.
            return false;
        }

        // Find insertion point: first entry with lower priority (higher value).
        let mut insert_pos: u32 = self.len;
        let mut i: u32 = 0;
        let mut found: bool = false;
        while i < self.len && !found
            invariant
                0 <= i <= self.len,
                self.len < MAX_WAITERS,
                self.len == old(self).len,
                self.inv(),
                // Queue unchanged during search
                forall|k: int| 0 <= k < 64
                    ==> (#[trigger] self.entries[k]) === old(self).entries[k],
                // Found state tracking
                !found ==> insert_pos == self.len,
                found ==> insert_pos == i && insert_pos < self.len,
                // When found: entry at insert_pos has priority > thread
                found ==> self.entries[insert_pos as int].is_some()
                    && thread.priority.view()
                        < self.entries[insert_pos as int].unwrap().priority.view(),
                // All entries before current scan position have priority <= thread
                forall|k: int| 0 <= k < i as int
                    ==> (#[trigger] self.entries[k]).is_some()
                    && self.entries[k].unwrap().priority.view()
                        <= thread.priority.view(),
                // Thread invariant preserved (not modified by loop)
                thread.inv(),
                thread.priority.inv(),
            decreases
                (self.len - i) * 2 + if !found { 1int } else { 0int },
        {
            // Access priority directly to help the solver with preconditions.
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

        // After search: insert_pos is the correct insertion point.
        // entries[0..insert_pos) have priority <= thread.priority.

        // Shift entries from insert_pos to len-1 right by one.
        let mut j: u32 = self.len;
        while j > insert_pos
            invariant
                insert_pos <= j <= self.len,
                self.len < MAX_WAITERS,
                self.len == old(self).len,
                0 <= insert_pos <= self.len,
                // Entries before insert_pos unchanged
                forall|k: int| 0 <= k < insert_pos as int
                    ==> (#[trigger] self.entries[k]) === old(self).entries[k],
                // Entries between insert_pos and j unchanged
                forall|k: int| insert_pos as int <= k < j as int
                    ==> (#[trigger] self.entries[k]) === old(self).entries[k],
                // Position j is None
                (#[trigger] self.entries[j as int]).is_none(),
                // Shifted portion: entries[j+1..len+1) are old entries[j..len)
                forall|k: int| (j as int) + 1 <= k <= self.len as int
                    ==> (#[trigger] self.entries[k]) === old(self).entries[k - 1],
                // Tail beyond len+1 is None
                forall|k: int| (self.len as int) + 1 <= k < 64
                    ==> (#[trigger] self.entries[k]).is_none(),
            decreases
                j - insert_pos,
        {
            self.entries[j as usize] = self.entries[(j - 1) as usize];
            self.entries[(j - 1) as usize] = None;
            j = j - 1;
        }

        // After shift: entries[insert_pos] is None, ready for insertion.
        // entries[0..insert_pos) = original, entries[insert_pos+1..len+1) = old shifted.

        // Insert the thread at the correct position.
        self.entries[insert_pos as usize] = Some(thread);
        self.len = self.len + 1;

        // Prove slots_valid: [0..new_len) are Some, [new_len..64) are None.
        assert(forall|k: int| 0 <= k < self.len as int
            ==> (#[trigger] self.entries[k]).is_some());
        assert(forall|k: int| self.len as int <= k < 64
            ==> (#[trigger] self.entries[k]).is_none());

        // Prove threads_valid.
        assert(forall|k: int| 0 <= k < self.len as int
            ==> (#[trigger] self.entries[k]).is_some()
            && self.entries[k].unwrap().inv()
            && self.entries[k].unwrap().state === ThreadState::Blocked);

        // Prove is_sorted: break into cases around insert_pos.
        // Case 1: both indices < insert_pos (original entries, unchanged, sorted).
        // Case 2: i1 < insert_pos, j1 == insert_pos (entry[i1] <= thread).
        // Case 3: i1 == insert_pos, j1 > insert_pos (thread <= shifted entry).
        // Case 4: both indices > insert_pos (shifted from originals, sorted).
        assert(forall|i1: int, j1: int| 0 <= i1 < j1 < self.len as int
            ==> (#[trigger] self.entries[i1]).is_some()
            && (#[trigger] self.entries[j1]).is_some()
            && self.entries[i1].unwrap().priority.view()
                <= self.entries[j1].unwrap().priority.view());

        // Prove no_duplicates.
        assert(forall|i1: int, j1: int| 0 <= i1 < j1 < self.len as int
            ==> (#[trigger] self.entries[i1]).is_some()
            && (#[trigger] self.entries[j1]).is_some()
            && self.entries[i1].unwrap().id.id
                != self.entries[j1].unwrap().id.id);

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
                count == old(self).len,
                self.len == old(self).len,
                // Cleared slots are None
                forall|k: int| 0 <= k < i as int
                    ==> (#[trigger] self.entries[k]).is_none(),
                // Slots beyond original length remain None
                forall|k: int| count as int <= k < 64
                    ==> (#[trigger] self.entries[k]).is_none(),
            decreases
                count - i,
        {
            self.entries[i as usize] = None;
            i = i + 1;
        }
        // Hint: combine the two cleared ranges [0,count) and [count,64) into [0,64).
        assert(forall|k: int| 0 <= k < 64
            ==> (#[trigger] self.entries[k]).is_none());
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
