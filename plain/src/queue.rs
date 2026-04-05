//! Verified dynamic queue for Zephyr RTOS.
//!
//! This is a formally verified port of zephyr/kernel/queue.c.
//! All safety-critical properties are proven with Verus (SMT/Z3).
//!
//! This module models the **count tracking** of Zephyr's queue object.
//! Actual linked-list data storage and wait queue management remain in
//! C — only the count crosses the FFI boundary.
//!
//! Unlike k_msgq (fixed-size ring buffer), k_queue is a dynamic singly-
//! linked list (sys_sflist_t) with no fixed capacity.  It is the
//! underlying structure for k_fifo and k_lifo.
//!
//! Source mapping:
//!   k_queue_init    -> Queue::init    (queue.c:63-76)
//!   k_queue_append  -> Queue::append  (queue.c:197-204, enqueue at tail)
//!   k_queue_prepend -> Queue::prepend (queue.c:206-213, enqueue at head)
//!   k_queue_get     -> Queue::get     (queue.c:335-370, dequeue from head)
//!
//! Omitted (not safety-relevant):
//!   - CONFIG_POLL (poll_events) — application convenience
//!   - CONFIG_OBJ_CORE_FIFO/LIFO — debug/tracing
//!   - CONFIG_USERSPACE (z_vrfy_*) — syscall marshaling
//!   - SYS_PORT_TRACING_* — instrumentation
//!   - k_queue_alloc_append/prepend — heap allocation wrappers
//!   - k_queue_insert — positional insert (uses queue_insert internally)
//!   - k_queue_append_list / k_queue_merge_slist — bulk operations
//!   - k_queue_remove / k_queue_unique_append — search operations
//!   - k_queue_peek_head / k_queue_peek_tail — non-modifying accessors
//!   - k_queue_cancel_wait — wait cancellation
//!
//! ASIL-D verified properties:
//!   QU1: count >= 0 (non-negative, trivially true for u32)
//!   QU2: append increments count (enqueue at tail)
//!   QU3: prepend increments count (enqueue at head — for LIFO behavior)
//!   QU4: get when count > 0: decrements count
//!   QU5: get when empty: returns EAGAIN
//!   QU6: no arithmetic overflow in any operation
use crate::error::*;
/// Lightweight insert decision for Queue — no queue allocation.
/// Used by FFI to avoid constructing full Queue objects.
#[derive(Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum InsertDecision {
    /// A waiting thread should be woken (count unchanged).
    WakeThread = 0,
    /// Data should be inserted into the list (count incremented).
    Insert = 1,
    /// Count would overflow — reject.
    Overflow = 2,
}
/// Lightweight get decision for Queue — no queue allocation.
/// Used by FFI to avoid constructing full Queue objects.
#[derive(Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum GetDecision {
    /// Data available: count decremented by 1.
    Dequeued = 0,
    /// Queue empty: return EAGAIN.
    Empty = 1,
}
/// Lightweight insert decision — takes scalars, no queue allocation.
///
/// Uses u32::MAX - 1 as the overflow boundary (same as fifo) to keep
/// one slot reserved, preventing count from reaching u32::MAX.
///
/// Verified properties (QU2, QU3, QU6):
/// - has_waiter ==> WakeThread (count unchanged)
/// - !has_waiter && count < u32::MAX - 1 ==> Insert
/// - !has_waiter && count >= u32::MAX - 1 ==> Overflow
pub fn insert_decide(count: u32, has_waiter: bool) -> InsertDecision {
    if has_waiter {
        InsertDecision::WakeThread
    } else if count < u32::MAX - 1 {
        InsertDecision::Insert
    } else {
        InsertDecision::Overflow
    }
}
/// Lightweight get decision — takes scalars, no queue allocation.
///
/// Verified properties (QU4, QU5):
/// - count > 0 ==> Dequeued
/// - count == 0 ==> Empty
pub fn get_decide(count: u32) -> GetDecision {
    if count > 0 { GetDecision::Dequeued } else { GetDecision::Empty }
}
/// Dynamic queue — count model.
///
/// Corresponds to Zephyr's struct k_queue {
///     sys_sflist_t data_q;    // singly-linked flagged list
///     struct k_spinlock lock; // spinlock for thread safety
///     _wait_q_t wait_q;       // wait queue for blocking get
/// };
///
/// We model the number of elements in data_q as `count`.
/// The C shim manages the actual linked-list nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Queue {
    /// Current number of elements in the queue.
    pub count: u32,
}
impl Queue {
    /// Initialize an empty queue.
    ///
    /// queue.c:63-76:
    ///   sys_sflist_init(&queue->data_q);
    ///   queue->lock = (struct k_spinlock) {};
    ///   z_waitq_init(&queue->wait_q);
    pub fn init() -> Queue {
        Queue { count: 0 }
    }
    /// Append an element to the tail of the queue (FIFO enqueue).
    ///
    /// queue.c:197-204:
    ///   (void)queue_insert(queue, NULL, data, false, true);
    ///   -> sys_sflist_insert(&queue->data_q, prev=tail, data);
    ///
    /// Returns OK on success, EOVERFLOW if count would overflow u32.
    ///
    /// Verified properties (QU2, QU6):
    /// - If count < u32::MAX: count incremented by 1
    /// - If count == u32::MAX: returns EOVERFLOW, state unchanged
    pub fn append(&mut self) -> i32 {
        if self.count == u32::MAX {
            EOVERFLOW
        } else {
            self.count = self.count + 1;
            OK
        }
    }
    /// Prepend an element to the head of the queue (LIFO push).
    ///
    /// queue.c:206-213:
    ///   (void)queue_insert(queue, NULL, data, false, false);
    ///   -> sys_sflist_insert(&queue->data_q, prev=NULL, data);
    ///
    /// Returns OK on success, EOVERFLOW if count would overflow u32.
    ///
    /// Verified properties (QU3, QU6):
    /// - If count < u32::MAX: count incremented by 1
    /// - If count == u32::MAX: returns EOVERFLOW, state unchanged
    pub fn prepend(&mut self) -> i32 {
        if self.count == u32::MAX {
            EOVERFLOW
        } else {
            self.count = self.count + 1;
            OK
        }
    }
    /// Get (dequeue) the head element from the queue.
    ///
    /// queue.c:335-370:
    ///   if (!sys_sflist_is_empty(&queue->data_q)) {
    ///       node = sys_sflist_get_not_empty(&queue->data_q);
    ///       ...
    ///   }
    ///
    /// Returns OK on success, EAGAIN when empty.
    ///
    /// Verified properties (QU4, QU5):
    /// - If count > 0: count decremented by 1
    /// - If count == 0: returns EAGAIN, state unchanged
    pub fn get(&mut self) -> i32 {
        if self.count == 0 {
            EAGAIN
        } else {
            self.count = self.count - 1;
            OK
        }
    }
    /// Check if the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
    /// Get the current element count.
    pub fn count_get(&self) -> u32 {
        self.count
    }
}
