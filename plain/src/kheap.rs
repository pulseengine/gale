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

use crate::error::*;

#[doc = " Kernel heap allocation tracker — byte count model."]
#[doc = ""]
#[doc = " Corresponds to Zephyr's struct k_heap {"]
#[doc = "     struct sys_heap heap;      // actual allocator (not modeled)"]
#[doc = "     _wait_q_t wait_q;          // blocking waiters (not modeled)"]
#[doc = "     struct k_spinlock lock;    // synchronization (not modeled)"]
#[doc = " };"]
#[doc = ""]
#[doc = " We model the byte-level accounting: allocated_bytes tracks total"]
#[doc = " bytes currently allocated. The C sys_heap manages the actual"]
#[doc = " free-list, coalescing, and alignment."]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KHeap {
    #[doc = " Maximum heap size in bytes (immutable after init)."]
    pub capacity: u32,
    #[doc = " Total bytes currently allocated."]
    pub allocated_bytes: u32,
}
impl KHeap {
    #[doc = " Initialize a kernel heap with the given capacity in bytes."]
    #[doc = ""]
    #[doc = " Corresponds to k_heap_init() (kheap.c:26-33)."]
    #[doc = " Returns EINVAL if capacity is 0."]
    pub fn init(capacity: u32) -> Result<KHeap, i32> {
        if capacity == 0 {
            Err(EINVAL)
        } else {
            Ok(KHeap {
                capacity,
                allocated_bytes: 0,
            })
        }
    }
    #[doc = " Allocate `bytes` from the heap."]
    #[doc = ""]
    #[doc = " Corresponds to k_heap_alloc() (kheap.c:119-129)."]
    #[doc = " KH2: success when space available, allocated_bytes += bytes."]
    #[doc = " KH3: returns ENOMEM when would exceed capacity."]
    #[doc = " KH6: no overflow — checked addition."]
    pub fn alloc(&mut self, bytes: u32) -> i32 {
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
    #[doc = " Free `bytes` back to the heap."]
    #[doc = ""]
    #[doc = " Corresponds to k_heap_free() (kheap.c:206-218)."]
    #[doc = " KH4: allocated_bytes -= bytes, with underflow protection."]
    pub fn free(&mut self, bytes: u32) -> i32 {
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
    #[doc = " Allocate with alignment requirement."]
    #[doc = ""]
    #[doc = " Corresponds to k_heap_aligned_alloc() (kheap.c:131-155)."]
    #[doc = " Models the byte accounting; actual alignment is handled by C's sys_heap."]
    #[doc = " The `align` parameter must be a power of 2 (or 0 for no alignment)."]
    #[doc = ""]
    #[doc = " For the model, alignment doesn't affect byte accounting — the"]
    #[doc = " underlying sys_heap handles alignment padding internally."]
    pub fn aligned_alloc(&mut self, bytes: u32, align: u32) -> i32 {
        self.alloc(bytes)
    }
    #[doc = " Allocate and zero-initialize memory for `num * size` bytes."]
    #[doc = ""]
    #[doc = " Corresponds to k_heap_calloc() (kheap.c:157-174)."]
    #[doc = " KH6: overflow check on num * size multiplication."]
    pub fn calloc(&mut self, num: u32, size: u32) -> i32 {
        let num64: u64 = num as u64;
        let size64: u64 = size as u64;
        #[allow(clippy::arithmetic_side_effects)]
        let total: u64 = num64 * size64;
        if total == 0 || total > u32::MAX as u64 {
            return ENOMEM;
        }
        let total_u32: u32 = total as u32;
        self.alloc(total_u32)
    }
    #[doc = " Number of bytes currently allocated."]
    pub fn allocated_get(&self) -> u32 {
        self.allocated_bytes
    }
    #[doc = " Number of free (available) bytes."]
    #[doc = " KH5: free_bytes + allocated_bytes == capacity."]
    pub fn free_get(&self) -> u32 {
        #[allow(clippy::arithmetic_side_effects)]
        let r = self.capacity - self.allocated_bytes;
        r
    }
    #[doc = " Total heap capacity in bytes."]
    pub fn capacity_get(&self) -> u32 {
        self.capacity
    }
    #[doc = " Check if the heap is full (all bytes allocated)."]
    pub fn is_full(&self) -> bool {
        self.allocated_bytes == self.capacity
    }
    #[doc = " Check if the heap is empty (no bytes allocated)."]
    pub fn is_empty(&self) -> bool {
        self.allocated_bytes == 0
    }
}
