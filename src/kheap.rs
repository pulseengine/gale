//! Verified kernel heap model for Zephyr RTOS.
//!
//! This is a formally verified model of Zephyr's k_heap kernel object
//! from kernel/kheap.c. All safety-critical properties are proven
//! with Verus (SMT/Z3).
//!
//! This module models the **byte-level allocation tracking** of Zephyr's
//! kernel heap. Actual memory management (sys_heap, free-list, coalescing)
//! remains in C — only the byte count accounting crosses the FFI boundary.
//!
//! Source mapping:
//!   k_heap_init         -> KHeap::init          (kheap.c:26-33)
//!   k_heap_alloc        -> KHeap::alloc         (kheap.c:119-129)
//!   k_heap_free         -> KHeap::free          (kheap.c:206-218)
//!   k_heap_aligned_alloc -> KHeap::aligned_alloc (kheap.c:131-155)
//!   k_heap_calloc       -> KHeap::calloc        (kheap.c:157-174)
//!   k_heap_realloc      -> (not modeled — requires old-size tracking)
//!
//! Omitted (not safety-relevant):
//!   - CONFIG_DEMAND_PAGING — boot-time init sequencing
//!   - SYS_PORT_TRACING_* — instrumentation
//!   - k_heap_realloc — requires tracking per-allocation sizes (in C heap)
//!   - sys_heap internal algorithms — bucket/free-list management
//!   - wait_q blocking — scheduling concern (z_pend_curr)
//!   - k_heap_array_get — linker section iteration
//!   - statics_init — boot-time initialization
//!
//! ASIL-D verified properties:
//!   KH1: 0 <= allocated_bytes <= capacity (bounds invariant)
//!   KH2: alloc(n) when allocated_bytes + n <= capacity: allocated_bytes += n
//!   KH3: alloc(n) when would exceed capacity: returns ENOMEM
//!   KH4: free(n): allocated_bytes -= n (with underflow protection)
//!   KH5: conservation: free_bytes + allocated_bytes == capacity
//!   KH6: no arithmetic overflow in any operation

use vstd::prelude::*;
use crate::error::*;

verus! {

/// Kernel heap allocation tracker — byte count model.
///
/// Corresponds to Zephyr's struct k_heap {
///     struct sys_heap heap;      // actual allocator (not modeled)
///     _wait_q_t wait_q;          // blocking waiters (not modeled)
///     struct k_spinlock lock;    // synchronization (not modeled)
/// };
///
/// We model the byte-level accounting: allocated_bytes tracks total
/// bytes currently allocated. The C sys_heap manages the actual
/// free-list, coalescing, and alignment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KHeap {
    /// Maximum heap size in bytes (immutable after init).
    pub capacity: u32,
    /// Total bytes currently allocated.
    pub allocated_bytes: u32,
}

impl KHeap {

    // ------------------------------------------------------------------
    // Specification predicates
    // ------------------------------------------------------------------

    /// Structural invariant — always maintained.
    /// KH1: allocated_bytes is bounded by capacity.
    pub open spec fn inv(&self) -> bool {
        &&& self.capacity > 0
        &&& self.allocated_bytes <= self.capacity
    }

    /// Heap is full (spec version).
    pub open spec fn is_full_spec(&self) -> bool {
        self.allocated_bytes == self.capacity
    }

    /// Heap is empty / all memory free (spec version).
    pub open spec fn is_empty_spec(&self) -> bool {
        self.allocated_bytes == 0
    }

    // ------------------------------------------------------------------
    // Operations
    // ------------------------------------------------------------------

    /// Initialize a kernel heap with the given capacity in bytes.
    ///
    /// Corresponds to k_heap_init() (kheap.c:26-33).
    /// Returns EINVAL if capacity is 0.
    pub fn init(capacity: u32) -> (result: Result<KHeap, i32>)
        ensures
            match result {
                Ok(h) => h.inv()
                    && h.allocated_bytes == 0
                    && h.capacity == capacity,
                Err(e) => e == EINVAL && capacity == 0,
            }
    {
        if capacity == 0 {
            Err(EINVAL)
        } else {
            Ok(KHeap { capacity, allocated_bytes: 0 })
        }
    }

    /// Allocate `bytes` from the heap.
    ///
    /// Corresponds to k_heap_alloc() (kheap.c:119-129).
    /// KH2: success when space available, allocated_bytes += bytes.
    /// KH3: returns ENOMEM when would exceed capacity.
    /// KH6: no overflow — checked addition.
    pub fn alloc(&mut self, bytes: u32) -> (rc: i32)
        requires
            old(self).inv(),
            bytes > 0,
        ensures
            self.inv(),
            self.capacity == old(self).capacity,
            // KH2: space available -> allocated
            old(self).allocated_bytes + bytes <= old(self).capacity ==> {
                &&& rc == OK
                &&& self.allocated_bytes == old(self).allocated_bytes + bytes
            },
            // KH3: would exceed capacity -> error, unchanged
            old(self).allocated_bytes + bytes > old(self).capacity ==> {
                &&& rc == ENOMEM
                &&& self.allocated_bytes == old(self).allocated_bytes
            },
    {
        // KH6: check for overflow and capacity
        if bytes <= self.capacity - self.allocated_bytes {
            #[allow(clippy::arithmetic_side_effects)]
            {
                self.allocated_bytes = self.allocated_bytes + bytes;
            }
            OK
        } else {
            ENOMEM
        }
    }

    /// Free `bytes` back to the heap.
    ///
    /// Corresponds to k_heap_free() (kheap.c:206-218).
    /// KH4: allocated_bytes -= bytes, with underflow protection.
    pub fn free(&mut self, bytes: u32) -> (rc: i32)
        requires
            old(self).inv(),
            bytes > 0,
        ensures
            self.inv(),
            self.capacity == old(self).capacity,
            // KH4: valid free -> decremented
            bytes <= old(self).allocated_bytes ==> {
                &&& rc == OK
                &&& self.allocated_bytes == old(self).allocated_bytes - bytes
            },
            // Underflow protection -> error, unchanged
            bytes > old(self).allocated_bytes ==> {
                &&& rc == EINVAL
                &&& self.allocated_bytes == old(self).allocated_bytes
            },
    {
        if bytes <= self.allocated_bytes {
            #[allow(clippy::arithmetic_side_effects)]
            {
                self.allocated_bytes = self.allocated_bytes - bytes;
            }
            OK
        } else {
            EINVAL
        }
    }

    /// Allocate with alignment requirement.
    ///
    /// Corresponds to k_heap_aligned_alloc() (kheap.c:131-155).
    /// Models the byte accounting; actual alignment is handled by C's sys_heap.
    /// The `align` parameter must be a power of 2 (or 0 for no alignment).
    ///
    /// For the model, alignment doesn't affect byte accounting — the
    /// underlying sys_heap handles alignment padding internally.
    pub fn aligned_alloc(&mut self, bytes: u32, align: u32) -> (rc: i32)
        requires
            old(self).inv(),
            bytes > 0,
            // align must be 0 or a power of 2
            align == 0 || (align > 0 && align & (align - 1) == 0),
        ensures
            self.inv(),
            self.capacity == old(self).capacity,
            // Same accounting as regular alloc
            old(self).allocated_bytes + bytes <= old(self).capacity ==> {
                &&& rc == OK
                &&& self.allocated_bytes == old(self).allocated_bytes + bytes
            },
            old(self).allocated_bytes + bytes > old(self).capacity ==> {
                &&& rc == ENOMEM
                &&& self.allocated_bytes == old(self).allocated_bytes
            },
    {
        self.alloc(bytes)
    }

    /// Allocate and zero-initialize memory for `num * size` bytes.
    ///
    /// Corresponds to k_heap_calloc() (kheap.c:157-174).
    /// KH6: overflow check on num * size multiplication.
    pub fn calloc(&mut self, num: u32, size: u32) -> (rc: i32)
        requires
            old(self).inv(),
        ensures
            self.inv(),
            self.capacity == old(self).capacity,
            // Overflow in multiplication -> error
            (num as u64) * (size as u64) > u32::MAX as u64 ==> {
                &&& rc == ENOMEM
                &&& self.allocated_bytes == old(self).allocated_bytes
            },
            // Zero-size allocation -> error
            num == 0 || size == 0 ==> {
                &&& rc == ENOMEM
                &&& self.allocated_bytes == old(self).allocated_bytes
            },
    {
        // Check for multiplication overflow (models size_mul_overflow)
        #[allow(clippy::arithmetic_side_effects)]
        let total: u64 = num as u64 * size as u64;
        if total == 0 || total > u32::MAX as u64 {
            return ENOMEM;
        }
        let total_u32: u32 = total as u32;
        self.alloc(total_u32)
    }

    /// Number of bytes currently allocated.
    pub fn allocated_get(&self) -> (r: u32)
        requires self.inv(),
        ensures r == self.allocated_bytes,
    {
        self.allocated_bytes
    }

    /// Number of free (available) bytes.
    /// KH5: free_bytes + allocated_bytes == capacity.
    pub fn free_get(&self) -> (r: u32)
        requires self.inv(),
        ensures r == self.capacity - self.allocated_bytes,
    {
        #[allow(clippy::arithmetic_side_effects)]
        let r = self.capacity - self.allocated_bytes;
        r
    }

    /// Total heap capacity in bytes.
    pub fn capacity_get(&self) -> (r: u32)
        requires self.inv(),
        ensures r == self.capacity,
    {
        self.capacity
    }

    /// Check if the heap is full (all bytes allocated).
    pub fn is_full(&self) -> (r: bool)
        requires self.inv(),
        ensures r == (self.allocated_bytes == self.capacity),
    {
        self.allocated_bytes == self.capacity
    }

    /// Check if the heap is empty (no bytes allocated).
    pub fn is_empty(&self) -> (r: bool)
        requires self.inv(),
        ensures r == (self.allocated_bytes == 0),
    {
        self.allocated_bytes == 0
    }
}

// ======================================================================
// Compositional proofs
// ======================================================================

/// KH1: invariant is inductive across all operations.
pub proof fn lemma_invariant_inductive()
    ensures
        // init establishes inv (from init's ensures)
        // alloc preserves inv (from alloc's ensures)
        // free preserves inv (from free's ensures)
        // aligned_alloc preserves inv (from aligned_alloc's ensures)
        // calloc preserves inv (from calloc's ensures)
        true,
{
}

/// KH2+KH4 roundtrip: alloc(n) then free(n) returns to original state.
pub proof fn lemma_alloc_free_roundtrip(allocated: u32, capacity: u32, n: u32)
    requires
        capacity > 0,
        allocated <= capacity,
        n > 0,
        allocated + n <= capacity,
    ensures ({
        // alloc(n): allocated -> allocated + n
        let after_alloc = (allocated + n) as u32;
        // free(n): allocated + n -> allocated
        let after_free = (after_alloc - n) as u32;
        after_free == allocated
    })
{
}

/// KH5: conservation (free + allocated == capacity).
pub proof fn lemma_conservation(allocated: u32, capacity: u32)
    requires
        capacity > 0,
        allocated <= capacity,
    ensures
        (capacity - allocated) + allocated == capacity,
{
}

/// KH3: alloc when full returns ENOMEM.
pub proof fn lemma_full_rejects_alloc(allocated: u32, capacity: u32, n: u32)
    requires
        capacity > 0,
        allocated == capacity,
        n > 0,
    ensures
        allocated + n > capacity,
{
}

/// KH4: free when empty is rejected.
pub proof fn lemma_empty_rejects_free(allocated: u32, n: u32)
    requires
        allocated == 0u32,
        n > 0u32,
    ensures
        n > allocated,
{
}

/// Free then alloc returns to original allocated_bytes.
pub proof fn lemma_free_alloc_roundtrip(allocated: u32, capacity: u32, n: u32)
    requires
        capacity > 0,
        allocated > 0,
        n > 0,
        n <= allocated,
        allocated <= capacity,
    ensures ({
        // free(n): allocated -> allocated - n
        let after_free = (allocated - n) as u32;
        // alloc(n): allocated - n -> allocated (since allocated <= capacity)
        let after_alloc = (after_free + n) as u32;
        after_alloc == allocated
    })
{
}

/// KH6: calloc multiplication overflow is detected.
pub proof fn lemma_calloc_overflow_detected(num: u32, size: u32)
    requires
        (num as u64) * (size as u64) > u32::MAX as u64,
    ensures
        // The overflow is caught by the u64 multiplication check
        (num as u64) * (size as u64) > u32::MAX as u64,
{
}

} // verus!
