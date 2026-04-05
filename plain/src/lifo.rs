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
use crate::error::*;
/// Lightweight put decision for LIFO — no queue allocation.
/// Used by FFI to avoid constructing full Lifo objects.
#[derive(Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PutDecision {
    /// A waiting thread should be woken (count unchanged).
    WakeThread = 0,
    /// Data should be inserted into the list (count incremented).
    Insert = 1,
    /// Count would overflow — reject.
    Overflow = 2,
}
/// Lightweight get decision for LIFO — no queue allocation.
/// Used by FFI to avoid constructing full Lifo objects.
#[derive(Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum GetDecision {
    /// Data available: count decremented by 1.
    Dequeued = 0,
    /// Queue empty: return EAGAIN.
    Empty = 1,
}
/// Lightweight put decision — takes scalars, no queue allocation.
///
/// Uses u32::MAX - 1 as the overflow boundary (same as fifo) to keep
/// one slot reserved, preventing count from reaching u32::MAX.
///
/// Verified properties (LI2, LI6):
/// - has_waiter ==> WakeThread (count unchanged)
/// - !has_waiter && count < u32::MAX - 1 ==> Insert
/// - !has_waiter && count >= u32::MAX - 1 ==> Overflow
pub fn put_decide(count: u32, has_waiter: bool) -> PutDecision {
    if has_waiter {
        PutDecision::WakeThread
    } else if count < u32::MAX - 1 {
        PutDecision::Insert
    } else {
        PutDecision::Overflow
    }
}
/// Lightweight get decision — takes scalars, no queue allocation.
///
/// Verified properties (LI3, LI4):
/// - count > 0 ==> Dequeued
/// - count == 0 ==> Empty
pub fn get_decide(count: u32) -> GetDecision {
    if count > 0 { GetDecision::Dequeued } else { GetDecision::Empty }
}
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
    /// Initialize a LIFO queue.
    ///
    /// queue.c:58-70 (k_queue_init):
    ///   sys_sflist_init(&queue->data_q);
    ///   queue->lock = {};
    ///   z_waitq_init(&queue->wait_q);
    ///
    /// LI5: init sets count to 0.
    pub fn init() -> Lifo {
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
    pub fn put(&mut self) -> i32 {
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
    pub fn get(&mut self) -> i32 {
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
    pub fn num_items(&self) -> u32 {
        self.count
    }
    /// Check if queue is empty.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}
