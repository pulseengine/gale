//! Verified LIFO queue for Zephyr RTOS.
//!
//! This is a formally verified model of zephyr/kernel/queue.c (LIFO mode).
//! All safety-critical properties are proven with Verus (SMT/Z3).
//!
//! This module models the **queue count** of Zephyr's k_lifo object.
//! k_lifo is a thin wrapper around k_queue using prepend (LIFO ordering).
//! Actual data storage, linked-list management, and wait queue handling
//! remain in C — only the count tracking crosses the FFI boundary.
//!
//! Source mapping (queue.c via k_lifo macros in kernel.h):
//!   k_lifo_init  -> Lifo::init   (queue.c:58-70, k_queue_init)
//!   k_lifo_put   -> Lifo::put    (queue.c:206-213, k_queue_prepend → queue_insert)
//!   k_lifo_get   -> Lifo::get    (queue.c:335-370, k_queue_get, K_NO_WAIT path)
//!
//! Omitted (not safety-relevant):
//!   - CONFIG_OBJ_CORE_LIFO — debug/tracing
//!   - CONFIG_USERSPACE (z_vrfy_*) — syscall marshaling
//!   - SYS_PORT_TRACING_* — instrumentation
//!   - k_lifo_alloc_put — heap allocation wrapper
//!   - CONFIG_POLL — poll event handling
//!   - Blocking get (timeout != K_NO_WAIT) — wait queue interaction
//!
//! Zephyr provenance:
//!   file: kernel/queue.c (shared with k_fifo)
//!   SHA-256: 4274e01743bfaf05ceec10a39a049f94a6ba1f040753a2c492976dcb0c69c1f3
//!   lines: 498
//!
//! ASIL-D verified properties:
//!   LI1: count >= 0 (non-negative, implicit in u32)
//!   LI2: put increments count by 1
//!   LI3: get when count > 0: decrements count by 1
//!   LI4: get when empty: returns EAGAIN, state unchanged
//!   LI5: init sets count to 0
//!   LI6: no arithmetic overflow in any operation

use vstd::prelude::*;
use crate::error::*;

verus! {

/// LIFO queue — count model.
///
/// Corresponds to Zephyr's struct k_lifo {
///     struct k_queue _queue;
/// };
///
/// k_queue internally uses sys_sflist (singly-linked flagged list).
/// We model the list length as `count`.
/// The C shim converts: count = sys_sflist_len(&queue->data_q).
///
/// Unlike k_stack, k_lifo has no fixed capacity — the linked list
/// grows dynamically. We guard against u32 overflow (LI6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lifo {
    /// Current number of items in the LIFO queue.
    pub count: u32,
}

impl Lifo {

    // ------------------------------------------------------------------
    // Specification predicates
    // ------------------------------------------------------------------

    /// Structural invariant — always maintained.
    /// For LIFO, count is simply a u32 (always >= 0).
    pub open spec fn inv(&self) -> bool {
        true  // u32 is always non-negative; no capacity bound
    }

    /// Queue is empty (spec version for verification).
    pub open spec fn is_empty_spec(&self) -> bool {
        self.count == 0
    }

    // ------------------------------------------------------------------
    // Operations
    // ------------------------------------------------------------------

    /// Initialize a LIFO queue.
    ///
    /// queue.c:58-70 (k_queue_init):
    ///   sys_sflist_init(&queue->data_q);
    ///   queue->lock = {};
    ///   z_waitq_init(&queue->wait_q);
    ///
    /// LI5: init sets count to 0.
    pub fn init() -> (result: Lifo)
        ensures
            result.inv(),
            result.count == 0,  // LI5
    {
        Lifo { count: 0 }
    }

    /// Add an element to the LIFO queue (prepend for LIFO ordering).
    ///
    /// queue.c:206-213 (k_queue_prepend → queue_insert):
    ///   sys_sflist_insert(&queue->data_q, prev, data);
    ///
    /// The underlying linked list has no capacity limit, but we must
    /// guard against u32::MAX overflow (LI6).
    ///
    /// Returns OK (count incremented) or EOVERFLOW (count at u32::MAX).
    pub fn put(&mut self) -> (rc: i32)
        requires old(self).inv(),
        ensures
            self.inv(),
            // LI2: count < MAX -> count incremented
            old(self).count < u32::MAX ==> {
                &&& rc == OK
                &&& self.count == old(self).count + 1
            },
            // LI6: overflow guard
            old(self).count >= u32::MAX ==> {
                &&& rc == EOVERFLOW
                &&& self.count == old(self).count
            },
    {
        if self.count >= u32::MAX {
            EOVERFLOW
        } else {
            #[allow(clippy::arithmetic_side_effects)]
            {
                self.count = self.count + 1;
            }
            OK
        }
    }

    /// Remove an element from the LIFO queue (most recently added).
    ///
    /// queue.c:335-370 (k_queue_get, K_NO_WAIT path):
    ///   if (!sys_sflist_is_empty(&queue->data_q)) {
    ///       node = sys_sflist_get_not_empty(&queue->data_q);
    ///       return z_queue_node_peek(node, true);
    ///   }
    ///   if (K_TIMEOUT_EQ(timeout, K_NO_WAIT)) { return NULL; }
    ///
    /// Returns OK (count decremented) or EAGAIN (empty, unchanged).
    pub fn get(&mut self) -> (rc: i32)
        requires old(self).inv(),
        ensures
            self.inv(),
            // LI3: not empty -> count decremented
            old(self).count > 0 ==> {
                &&& rc == OK
                &&& self.count == old(self).count - 1
            },
            // LI4: empty -> EAGAIN, state unchanged
            old(self).count == 0 ==> {
                &&& rc == EAGAIN
                &&& self.count == old(self).count
            },
    {
        if self.count == 0 {
            EAGAIN
        } else {
            #[allow(clippy::arithmetic_side_effects)]
            {
                self.count = self.count - 1;
            }
            OK
        }
    }

    /// Number of items in the queue.
    pub fn num_items(&self) -> (r: u32)
        requires self.inv(),
        ensures r == self.count,
    {
        self.count
    }

    /// Check if queue is empty.
    pub fn is_empty(&self) -> (r: bool)
        requires self.inv(),
        ensures r == (self.count == 0),
    {
        self.count == 0
    }
}

// ======================================================================
// Compositional proofs
// ======================================================================

/// LI1/LI5: invariant is inductive across all operations.
/// The ensures clauses on put/get already prove this; this lemma
/// documents the property.
pub proof fn lemma_invariant_inductive()
    ensures
        // init establishes inv (from init's ensures)
        // put preserves inv (from put's ensures)
        // get preserves inv (from get's ensures)
        true,
{
}

/// LI2+LI3: put then get returns to original count.
pub proof fn lemma_put_get_roundtrip(count: u32)
    requires
        count < u32::MAX,
    ensures ({
        // put: count -> count + 1
        let after_put = (count + 1) as u32;
        // get: count + 1 -> count (since count + 1 > 0)
        let after_get = (after_put - 1) as u32;
        after_get == count
    })
{
}

/// LI4: empty queue rejects get.
pub proof fn lemma_empty_rejects_get(count: u32)
    requires
        count == 0u32,
    ensures
        count == 0u32,
{
}

/// LI6: full queue (u32::MAX) rejects put.
pub proof fn lemma_overflow_rejects_put(count: u32)
    requires
        count == u32::MAX,
    ensures
        count >= u32::MAX,
{
}

/// Get then put returns to original count.
pub proof fn lemma_get_put_roundtrip(count: u32)
    requires
        count > 0,
    ensures ({
        // get: count -> count - 1
        let after_get = (count - 1) as u32;
        // put: count - 1 -> count (since count - 1 < u32::MAX when count > 0)
        let after_put = (after_get + 1) as u32;
        after_put == count
    })
{
}

/// Multiple puts increase count monotonically.
pub proof fn lemma_put_monotonic(count: u32, n: u32)
    requires
        n > 0,
        count <= u32::MAX - n,
    ensures
        (count + n) as u32 > count,
{
}

} // verus!
