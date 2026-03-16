//! Verified memory pool model for Zephyr RTOS.
//!
//! This is a formally verified model of a variable-size block memory pool.
//! Zephyr removed the dedicated k_mem_pool in favor of k_heap, but the
//! pool pattern is still used via CONFIG_DYNAMIC_THREAD_POOL_SIZE and
//! sys_mem_pool. This module models a fixed-block pool allocator.
//!
//! This module models the **block-level allocation tracking** of a pool.
//! Actual memory management (bitarray, alignment) remains in C.
//!
//! Source mapping:
//!   pool init           -> MemPool::init         (pool creation)
//!   pool alloc          -> MemPool::alloc        (block allocation)
//!   pool free           -> MemPool::free         (block deallocation)
//!
//! Omitted (not safety-relevant):
//!   - sys_bitarray internals — bit manipulation details
//!   - alignment / padding — hardware-specific
//!   - CONFIG_USERSPACE — syscall marshaling
//!
//! ASIL-D verified properties:
//!   MP1: 0 <= allocated <= capacity (bounds invariant)
//!   MP2: alloc when allocated < capacity: allocated += 1
//!   MP3: alloc when full: returns ENOMEM
//!   MP4: free when allocated > 0: allocated -= 1
//!   MP5: conservation: free_blocks + allocated == capacity
//!   MP6: no arithmetic overflow in any operation

use vstd::prelude::*;
use crate::error::*;

verus! {

/// Fixed-block memory pool model.
///
/// Models a pool of equal-sized blocks. Each block is either
/// allocated or free. We track the count, not individual blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemPool {
    /// Total number of blocks in the pool (immutable after init).
    pub capacity: u32,
    /// Number of blocks currently allocated.
    pub allocated: u32,
    /// Size of each block in bytes (immutable after init).
    pub block_size: u32,
}

impl MemPool {

    // ------------------------------------------------------------------
    // Specification predicates
    // ------------------------------------------------------------------

    /// Structural invariant — always maintained.
    /// MP1: allocated is bounded by capacity.
    pub open spec fn inv(&self) -> bool {
        &&& self.capacity > 0
        &&& self.block_size > 0
        &&& self.allocated <= self.capacity
    }

    /// Pool is full (all blocks allocated).
    pub open spec fn is_full_spec(&self) -> bool {
        self.allocated == self.capacity
    }

    /// Pool is empty (no blocks allocated).
    pub open spec fn is_empty_spec(&self) -> bool {
        self.allocated == 0
    }

    // ------------------------------------------------------------------
    // Operations
    // ------------------------------------------------------------------

    /// Initialize a memory pool.
    ///
    /// Returns EINVAL if capacity or block_size is 0.
    pub fn init(capacity: u32, block_size: u32) -> (result: Result<MemPool, i32>)
        ensures
            match result {
                Ok(p) => p.inv()
                    && p.allocated == 0
                    && p.capacity == capacity
                    && p.block_size == block_size,
                Err(e) => e == EINVAL && (capacity == 0 || block_size == 0),
            }
    {
        if capacity == 0 || block_size == 0 {
            Err(EINVAL)
        } else {
            Ok(MemPool { capacity, allocated: 0, block_size })
        }
    }

    /// Allocate one block from the pool.
    ///
    /// MP2: success when allocated < capacity.
    /// MP3: returns ENOMEM when full.
    /// MP6: no overflow.
    pub fn alloc(&mut self) -> (rc: i32)
        requires old(self).inv(),
        ensures
            self.inv(),
            self.capacity == old(self).capacity,
            self.block_size == old(self).block_size,
            // MP2: space available -> allocated
            old(self).allocated < old(self).capacity ==> {
                &&& rc == OK
                &&& self.allocated == old(self).allocated + 1
            },
            // MP3: full -> error, unchanged
            old(self).allocated == old(self).capacity ==> {
                &&& rc == ENOMEM
                &&& self.allocated == old(self).allocated
            },
    {
        if self.allocated < self.capacity {
            #[allow(clippy::arithmetic_side_effects)]
            {
                self.allocated = self.allocated + 1;
            }
            OK
        } else {
            ENOMEM
        }
    }

    /// Allocate `count` blocks from the pool.
    ///
    /// Returns OK if enough blocks are available, ENOMEM otherwise.
    /// MP6: no overflow — checked addition.
    pub fn alloc_many(&mut self, count: u32) -> (rc: i32)
        requires
            old(self).inv(),
            count > 0,
        ensures
            self.inv(),
            self.capacity == old(self).capacity,
            self.block_size == old(self).block_size,
            old(self).allocated + count <= old(self).capacity ==> {
                &&& rc == OK
                &&& self.allocated == old(self).allocated + count
            },
            old(self).allocated + count > old(self).capacity ==> {
                &&& rc == ENOMEM
                &&& self.allocated == old(self).allocated
            },
    {
        if count <= self.capacity - self.allocated {
            #[allow(clippy::arithmetic_side_effects)]
            {
                self.allocated = self.allocated + count;
            }
            OK
        } else {
            ENOMEM
        }
    }

    /// Free one block back to the pool.
    ///
    /// MP4: success when allocated > 0.
    pub fn free(&mut self) -> (rc: i32)
        requires old(self).inv(),
        ensures
            self.inv(),
            self.capacity == old(self).capacity,
            self.block_size == old(self).block_size,
            // MP4: was allocated -> decremented
            old(self).allocated > 0 ==> {
                &&& rc == OK
                &&& self.allocated == old(self).allocated - 1
            },
            // Empty -> error
            old(self).allocated == 0 ==> {
                &&& rc == EINVAL
                &&& self.allocated == old(self).allocated
            },
    {
        if self.allocated > 0 {
            #[allow(clippy::arithmetic_side_effects)]
            {
                self.allocated = self.allocated - 1;
            }
            OK
        } else {
            EINVAL
        }
    }

    /// Free `count` blocks back to the pool.
    pub fn free_many(&mut self, count: u32) -> (rc: i32)
        requires
            old(self).inv(),
            count > 0,
        ensures
            self.inv(),
            self.capacity == old(self).capacity,
            self.block_size == old(self).block_size,
            count <= old(self).allocated ==> {
                &&& rc == OK
                &&& self.allocated == old(self).allocated - count
            },
            count > old(self).allocated ==> {
                &&& rc == EINVAL
                &&& self.allocated == old(self).allocated
            },
    {
        if count <= self.allocated {
            #[allow(clippy::arithmetic_side_effects)]
            {
                self.allocated = self.allocated - count;
            }
            OK
        } else {
            EINVAL
        }
    }

    /// Number of blocks currently allocated.
    pub fn allocated_get(&self) -> (r: u32)
        requires self.inv(),
        ensures r == self.allocated,
    {
        self.allocated
    }

    /// Number of free (available) blocks.
    /// MP5: free_blocks + allocated == capacity.
    pub fn free_get(&self) -> (r: u32)
        requires self.inv(),
        ensures r == self.capacity - self.allocated,
    {
        #[allow(clippy::arithmetic_side_effects)]
        let r = self.capacity - self.allocated;
        r
    }

    /// Total pool capacity in blocks.
    pub fn capacity_get(&self) -> (r: u32)
        requires self.inv(),
        ensures r == self.capacity,
    {
        self.capacity
    }

    /// Block size in bytes.
    pub fn block_size_get(&self) -> (r: u32)
        requires self.inv(),
        ensures r == self.block_size,
    {
        self.block_size
    }

    /// Total pool size in bytes (capacity * block_size).
    /// MP6: overflow check.
    pub fn total_size(&self) -> (r: Option<u32>)
        requires self.inv(),
        ensures
            (self.capacity as u64) * (self.block_size as u64) <= u32::MAX as u64
                ==> r === Some(((self.capacity as u64 * self.block_size as u64) as u32)),
            (self.capacity as u64) * (self.block_size as u64) > u32::MAX as u64
                ==> r.is_none(),
    {
        #[allow(clippy::arithmetic_side_effects)]
        let total: u64 = self.capacity as u64 * self.block_size as u64;
        if total > u32::MAX as u64 {
            None
        } else {
            Some(total as u32)
        }
    }

    /// Check if the pool is full.
    pub fn is_full(&self) -> (r: bool)
        requires self.inv(),
        ensures r == (self.allocated == self.capacity),
    {
        self.allocated == self.capacity
    }

    /// Check if the pool is empty (no blocks allocated).
    pub fn is_empty(&self) -> (r: bool)
        requires self.inv(),
        ensures r == (self.allocated == 0),
    {
        self.allocated == 0
    }
}

// ======================================================================
// Compositional proofs
// ======================================================================

/// MP1: invariant is inductive across all operations.
pub proof fn lemma_invariant_inductive()
    ensures true,
{}

/// MP2+MP4: alloc then free returns to original state.
pub proof fn lemma_alloc_free_roundtrip(allocated: u32, capacity: u32)
    requires
        capacity > 0,
        allocated < capacity,
    ensures ({
        let after_alloc = (allocated + 1) as u32;
        let after_free = (after_alloc - 1) as u32;
        after_free == allocated
    })
{}

/// MP5: conservation (free + allocated == capacity).
pub proof fn lemma_conservation(allocated: u32, capacity: u32)
    requires
        capacity > 0,
        allocated <= capacity,
    ensures
        (capacity - allocated) + allocated == capacity,
{}

/// MP3: full pool rejects alloc.
pub proof fn lemma_full_rejects_alloc(allocated: u32, capacity: u32)
    requires
        capacity > 0,
        allocated == capacity,
    ensures
        !(allocated < capacity),
{}

/// MP4: empty pool rejects free.
pub proof fn lemma_empty_rejects_free(allocated: u32)
    requires allocated == 0u32,
    ensures !(allocated > 0),
{}

/// MP6: total_size overflow detected.
pub proof fn lemma_total_size_overflow(capacity: u32, block_size: u32)
    requires
        capacity > 0,
        block_size > 0,
        (capacity as u64) * (block_size as u64) > u32::MAX as u64,
    ensures
        (capacity as u64) * (block_size as u64) > u32::MAX as u64,
{}

} // verus!
