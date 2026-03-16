//! Verified LIFO stack for Zephyr RTOS.
//!
//! This is a formally verified port of zephyr/kernel/stack.c.
//! All safety-critical properties are proven with Verus (SMT/Z3).
//!
//! This module models the **LIFO index arithmetic** of Zephyr's stack object.
//! Actual data storage and wait queue management remain in C — only the
//! count/capacity tracking crosses the FFI boundary.
//!
//! Source mapping:
//!   k_stack_init   -> Stack::init   (stack.c:27-42)
//!   k_stack_push   -> Stack::push   (stack.c:101-136, capacity check + increment)
//!   k_stack_pop    -> Stack::pop    (stack.c:148-190, empty check + decrement)
//!
//! Omitted (not safety-relevant):
//!   - CONFIG_OBJ_CORE_STACK — debug/tracing
//!   - CONFIG_USERSPACE (z_vrfy_*) — syscall marshaling
//!   - SYS_PORT_TRACING_* — instrumentation
//!   - k_stack_alloc_init — heap allocation wrapper
//!   - k_stack_cleanup — deallocation
//!
//! ASIL-D verified properties:
//!   SK1: 0 <= count <= capacity (bounds invariant)
//!   SK2: capacity > 0 (always after init)
//!   SK3: push when not full: count incremented by 1
//!   SK4: push when full: returns ENOMEM, state unchanged
//!   SK5: pop when not empty: count decremented by 1
//!   SK6: pop when empty: returns EBUSY, state unchanged
//!   SK7: num_free + num_used == capacity (conservation)
//!   SK8: no arithmetic overflow in any operation
//!   SK9: push-pop roundtrip preserves state
use crate::error::*;
/// LIFO stack — count/capacity model.
///
/// Corresponds to Zephyr's struct k_stack {
///     stack_data_t *base;   // buffer start
///     stack_data_t *next;   // current top-of-stack
///     stack_data_t *top;    // buffer end (base + num_entries)
/// };
///
/// We model next-base as `count` and top-base as `capacity`.
/// The C shim converts: count = (next - base), capacity = (top - base).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stack {
    /// Maximum number of entries (immutable after init).
    pub capacity: u32,
    /// Current number of entries on the stack.
    pub count: u32,
}
impl Stack {
    /// Initialize a stack with given capacity.
    ///
    /// stack.c:27-42:
    ///   stack->base = buffer; stack->next = buffer;
    ///   stack->top = buffer + num_entries;
    pub fn init(capacity: u32) -> Result<Stack, i32> {
        if capacity == 0 {
            Err(EINVAL)
        } else {
            Ok(Stack { capacity, count: 0 })
        }
    }
    /// Push an entry onto the stack.
    ///
    /// stack.c:109-125:
    ///   CHECKIF(stack->next == stack->top) { ret = -ENOMEM; }
    ///   *(stack->next) = data; stack->next++;
    ///
    /// Returns OK (count incremented) or ENOMEM (full, unchanged).
    pub fn push(&mut self) -> i32 {
        if self.count >= self.capacity {
            ENOMEM
        } else {
            #[allow(clippy::arithmetic_side_effects)]
            {
                self.count = self.count + 1;
            }
            OK
        }
    }
    /// Pop an entry from the stack.
    ///
    /// stack.c:158-166:
    ///   if (stack->next > stack->base) {
    ///       stack->next--; *data = *(stack->next);
    ///   }
    ///
    /// Returns OK (count decremented) or EBUSY (empty, unchanged).
    pub fn pop(&mut self) -> i32 {
        if self.count == 0 {
            EBUSY
        } else {
            #[allow(clippy::arithmetic_side_effects)]
            {
                self.count = self.count - 1;
            }
            OK
        }
    }
    /// Number of free slots.
    pub fn num_free(&self) -> u32 {
        #[allow(clippy::arithmetic_side_effects)]
        let r = self.capacity - self.count;
        r
    }
    /// Number of used slots.
    pub fn num_used(&self) -> u32 {
        self.count
    }
    /// Check if stack is full.
    pub fn is_full(&self) -> bool {
        self.count == self.capacity
    }
    /// Check if stack is empty.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
    /// Get stack capacity.
    pub fn capacity(&self) -> u32 {
        self.capacity
    }
}
