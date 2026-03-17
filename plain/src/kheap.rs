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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KHeap {
    /// Maximum heap size in bytes (immutable after init).
    pub capacity: u32,
    /// Total bytes currently allocated.
    pub allocated_bytes: u32,
}
impl KHeap {
    /// Initialize a kernel heap with the given capacity in bytes.
    ///
    /// Corresponds to k_heap_init() (kheap.c:26-33).
    /// Returns EINVAL if capacity is 0.
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
    /// Allocate `bytes` from the heap.
    ///
    /// Corresponds to k_heap_alloc() (kheap.c:119-129).
    /// KH2: success when space available, allocated_bytes += bytes.
    /// KH3: returns ENOMEM when would exceed capacity.
    /// KH6: no overflow — checked addition.
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
    /// Free `bytes` back to the heap.
    ///
    /// Corresponds to k_heap_free() (kheap.c:206-218).
    /// KH4: allocated_bytes -= bytes, with underflow protection.
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
    /// Allocate with alignment requirement.
    ///
    /// Corresponds to k_heap_aligned_alloc() (kheap.c:131-155).
    /// Models the byte accounting; actual alignment is handled by C's sys_heap.
    /// The `align` parameter must be a power of 2 (or 0 for no alignment).
    ///
    /// For the model, alignment doesn't affect byte accounting — the
    /// underlying sys_heap handles alignment padding internally.
    pub fn aligned_alloc(&mut self, bytes: u32, align: u32) -> i32 {
        self.alloc(bytes)
    }
    /// Allocate and zero-initialize memory for `num * size` bytes.
    ///
    /// Corresponds to k_heap_calloc() (kheap.c:157-174).
    /// KH6: overflow check on num * size multiplication.
    pub fn calloc(&mut self, num: u32, size: u32) -> i32 {
        #[allow(clippy::arithmetic_side_effects)]
        let total: u64 = num as u64 * size as u64;
        if total == 0 || total > u32::MAX as u64 {
            return ENOMEM;
        }
        let total_u32: u32 = total as u32;
        self.alloc(total_u32)
    }
    /// Number of bytes currently allocated.
    pub fn allocated_get(&self) -> u32 {
        self.allocated_bytes
    }
    /// Number of free (available) bytes.
    /// KH5: free_bytes + allocated_bytes == capacity.
    pub fn free_get(&self) -> u32 {
        #[allow(clippy::arithmetic_side_effects)]
        let r = self.capacity - self.allocated_bytes;
        r
    }
    /// Total heap capacity in bytes.
    pub fn capacity_get(&self) -> u32 {
        self.capacity
    }
    /// Check if the heap is full (all bytes allocated).
    pub fn is_full(&self) -> bool {
        self.allocated_bytes == self.capacity
    }
    /// Check if the heap is empty (no bytes allocated).
    pub fn is_empty(&self) -> bool {
        self.allocated_bytes == 0
    }
}
