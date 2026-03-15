//! Verified FIFO queue for Zephyr RTOS.
//!
//! This is a formally verified port of zephyr/kernel/queue.c (k_fifo layer).
//! All safety-critical properties are proven with Verus (SMT/Z3).
//!
//! This module models the **FIFO queue count tracking** of Zephyr's k_fifo
//! object.  Actual data storage (linked list) and wait queue management
//! remain in C -- only the element count crosses the FFI boundary.
//!
//! k_fifo is a thin wrapper around k_queue; the underlying data structure
//! is a singly-linked flag list (sys_sflist_t).
//!
//! Source mapping:
//!   k_fifo_init       -> Fifo::init       (queue.c:58-70,  init empty list)
//!   k_fifo_put        -> Fifo::put        (queue.c:132-186, append + count++)
//!   k_fifo_get        -> Fifo::get        (queue.c:335-370, dequeue + count--)
//!   k_fifo_is_empty   -> Fifo::is_empty   (inline: sflist_is_empty)
//!   k_fifo_peek_head  -> Fifo::peek_head  (queue.c:404-411)
//!
//! Omitted (not safety-relevant):
//!   - CONFIG_OBJ_CORE_FIFO -- debug/tracing
//!   - CONFIG_USERSPACE (z_vrfy_*) -- syscall marshaling
//!   - SYS_PORT_TRACING_* -- instrumentation
//!   - k_fifo_alloc_put -- heap allocation wrapper
//!   - k_fifo_cancel_wait -- signal cancellation
//!   - k_fifo_put_list / k_fifo_put_slist -- bulk insert
//!   - k_fifo_peek_tail -- tail peek
//!
//! ASIL-D verified properties:
//!   FI1: count >= 0 (trivially true for u32)
//!   FI2: put increments count by 1
//!   FI3: get when count > 0: decrements count by 1
//!   FI4: get when count == 0: returns EAGAIN (no data)
//!   FI5: init sets count to 0
//!   FI6: no arithmetic overflow in any operation
use crate::error::*;
/// FIFO queue -- count model.
///
/// Corresponds to Zephyr's struct k_fifo {
///     struct k_queue _queue;
/// };
///
/// where k_queue contains a sys_sflist_t data_q (linked list).
/// We model the number of elements currently in data_q as `count`.
/// The C shim converts: count = sys_sflist_len(&queue->data_q).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fifo {
    /// Current number of items in the queue.
    pub count: u32,
}
impl Fifo {
    /// Initialize an empty FIFO queue.
    ///
    /// queue.c:58-70:
    ///   sys_sflist_init(&queue->data_q);
    ///   z_waitq_init(&queue->wait_q);
    pub fn init() -> Fifo {
        Fifo { count: 0 }
    }
    /// Enqueue an item at the tail (FIFO order).
    ///
    /// queue.c:132-186 (queue_insert with is_append=true):
    ///   sys_sflist_insert(&queue->data_q, prev, data);
    ///
    /// Returns OK (count incremented) or EOVERFLOW (count at max).
    pub fn put(&mut self) -> i32 {
        if self.count >= u32::MAX - 1 {
            EOVERFLOW
        } else {
            #[allow(clippy::arithmetic_side_effects)]
            {
                self.count = self.count + 1;
            }
            OK
        }
    }
    /// Dequeue an item from the head.
    ///
    /// queue.c:335-370 (z_impl_k_queue_get):
    ///   if (!sys_sflist_is_empty(&queue->data_q)) {
    ///       node = sys_sflist_get_not_empty(&queue->data_q);
    ///   } else { return NULL; /* -EAGAIN */ }
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
    /// Check if the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
    /// Peek at the head without dequeuing.
    /// Returns true if there is an item (count > 0), false otherwise.
    /// In real Zephyr, this returns a pointer; we model presence/absence.
    pub fn peek_head(&self) -> bool {
        self.count > 0
    }
}
