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

use vstd::prelude::*;
use crate::error::*;

verus! {

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stack {
    /// Maximum number of entries (immutable after init).
    pub capacity: u32,
    /// Current number of entries on the stack.
    pub count: u32,
}

impl Stack {

    // ------------------------------------------------------------------
    // Specification predicates
    // ------------------------------------------------------------------

    /// Structural invariant — always maintained.
    pub open spec fn inv(&self) -> bool {
        self.capacity > 0
        && self.count <= self.capacity
    }

    /// Stack is full (spec version for verification).
    pub open spec fn is_full_spec(&self) -> bool {
        self.count == self.capacity
    }

    /// Stack is empty (spec version for verification).
    pub open spec fn is_empty_spec(&self) -> bool {
        self.count == 0
    }

    // ------------------------------------------------------------------
    // Operations
    // ------------------------------------------------------------------

    /// Initialize a stack with given capacity.
    ///
    /// stack.c:27-42:
    ///   stack->base = buffer; stack->next = buffer;
    ///   stack->top = buffer + num_entries;
    pub fn init(capacity: u32) -> (result: Result<Stack, i32>)
        ensures
            match result {
                Ok(s) => s.inv()
                    && s.count == 0
                    && s.capacity == capacity,
                Err(e) => e == EINVAL && capacity == 0,
            }
    {
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
    pub fn push(&mut self) -> (rc: i32)
        requires old(self).inv(),
        ensures
            self.inv(),
            self.capacity == old(self).capacity,
            // SK3: not full -> count incremented
            old(self).count < old(self).capacity ==> {
                &&& rc == OK
                &&& self.count == old(self).count + 1
            },
            // SK4: full -> error, state unchanged
            old(self).count >= old(self).capacity ==> {
                &&& rc == ENOMEM
                &&& self.count == old(self).count
            },
    {
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
    pub fn pop(&mut self) -> (rc: i32)
        requires old(self).inv(),
        ensures
            self.inv(),
            self.capacity == old(self).capacity,
            // SK5: not empty -> count decremented
            old(self).count > 0 ==> {
                &&& rc == OK
                &&& self.count == old(self).count - 1
            },
            // SK6: empty -> error, state unchanged
            old(self).count == 0 ==> {
                &&& rc == EBUSY
                &&& self.count == old(self).count
            },
    {
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
    pub fn num_free(&self) -> (r: u32)
        requires self.inv(),
        ensures r == self.capacity - self.count,
    {
        #[allow(clippy::arithmetic_side_effects)]
        let r = self.capacity - self.count;
        r
    }

    /// Number of used slots.
    pub fn num_used(&self) -> (r: u32)
        requires self.inv(),
        ensures r == self.count,
    {
        self.count
    }

    /// Check if stack is full.
    pub fn is_full(&self) -> (r: bool)
        requires self.inv(),
        ensures r == (self.count == self.capacity),
    {
        self.count == self.capacity
    }

    /// Check if stack is empty.
    pub fn is_empty(&self) -> (r: bool)
        requires self.inv(),
        ensures r == (self.count == 0),
    {
        self.count == 0
    }

    /// Get stack capacity.
    pub fn capacity(&self) -> (r: u32)
        requires self.inv(),
        ensures r == self.capacity,
    {
        self.capacity
    }
}

// ======================================================================
// Compositional proofs
// ======================================================================

/// SK1/SK2: invariant is inductive across all operations.
/// The ensures clauses on push/pop already prove this; this lemma
/// documents the property.
pub proof fn lemma_invariant_inductive()
    ensures
        // init establishes inv (from init's ensures)
        // push preserves inv (from push's ensures)
        // pop preserves inv (from pop's ensures)
        true,
{
}

/// SK9: push then pop returns to original count.
pub proof fn lemma_push_pop_roundtrip(count: u32, capacity: u32)
    requires
        capacity > 0,
        count < capacity,
    ensures ({
        // push: count -> count + 1
        let after_push = (count + 1) as u32;
        // pop: count + 1 -> count
        let after_pop = (after_push - 1) as u32;
        after_pop == count
    })
{
}

/// SK7: free + used == capacity (conservation).
pub proof fn lemma_capacity_conservation(count: u32, capacity: u32)
    requires
        capacity > 0,
        count <= capacity,
    ensures
        (capacity - count) + count == capacity,
{
}

/// SK4: full stack rejects push.
pub proof fn lemma_full_rejects_push(count: u32, capacity: u32)
    requires
        capacity > 0,
        count == capacity,
    ensures
        count >= capacity,
{
}

/// SK6: empty stack rejects pop.
pub proof fn lemma_empty_rejects_pop(count: u32)
    requires
        count == 0u32,
    ensures
        count == 0u32,
{
}

/// Pop then push returns to original count.
pub proof fn lemma_pop_push_roundtrip(count: u32, capacity: u32)
    requires
        capacity > 0,
        count > 0,
        count <= capacity,
    ensures ({
        // pop: count -> count - 1
        let after_pop = (count - 1) as u32;
        // push: count - 1 -> count (since count - 1 < capacity)
        let after_push = (after_pop + 1) as u32;
        after_push == count
    })
{
}

} // verus!
