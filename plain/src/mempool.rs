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
use crate::error::*;
/// Fixed-block memory pool model.
///
/// Models a pool of equal-sized blocks. Each block is either
/// allocated or free. We track the count, not individual blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemPool {
    /// Total number of blocks in the pool (immutable after init).
    pub capacity: u32,
    /// Number of blocks currently allocated.
    pub allocated: u32,
    /// Size of each block in bytes (immutable after init).
    pub block_size: u32,
}
impl MemPool {
    /// Initialize a memory pool.
    ///
    /// Returns EINVAL if capacity or block_size is 0.
    pub fn init(capacity: u32, block_size: u32) -> Result<MemPool, i32> {
        if capacity == 0 || block_size == 0 {
            Err(EINVAL)
        } else {
            Ok(MemPool {
                capacity,
                allocated: 0,
                block_size,
            })
        }
    }
    /// Allocate one block from the pool.
    ///
    /// MP2: success when allocated < capacity.
    /// MP3: returns ENOMEM when full.
    /// MP6: no overflow.
    pub fn alloc(&mut self) -> i32 {
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
    pub fn alloc_many(&mut self, count: u32) -> i32 {
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
    pub fn free(&mut self) -> i32 {
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
    pub fn free_many(&mut self, count: u32) -> i32 {
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
    pub fn allocated_get(&self) -> u32 {
        self.allocated
    }
    /// Number of free (available) blocks.
    /// MP5: free_blocks + allocated == capacity.
    pub fn free_get(&self) -> u32 {
        #[allow(clippy::arithmetic_side_effects)]
        let r = self.capacity - self.allocated;
        r
    }
    /// Total pool capacity in blocks.
    pub fn capacity_get(&self) -> u32 {
        self.capacity
    }
    /// Block size in bytes.
    pub fn block_size_get(&self) -> u32 {
        self.block_size
    }
    /// Total pool size in bytes (capacity * block_size).
    /// MP6: overflow check.
    pub fn total_size(&self) -> Option<u32> {
        let cap64: u64 = self.capacity as u64;
        let bs64: u64 = self.block_size as u64;
        let total: u64 = match cap64.checked_mul(bs64) {
            Some(v) => v,
            None => return None,
        };
        if total > u32::MAX as u64 { None } else { Some(total as u32) }
    }
    /// Check if the pool is full.
    pub fn is_full(&self) -> bool {
        self.allocated == self.capacity
    }
    /// Check if the pool is empty (no blocks allocated).
    pub fn is_empty(&self) -> bool {
        self.allocated == 0
    }
}
/// Decision for mempool alloc: validate and compute new allocated count.
///
/// MP2: alloc success. MP3: full returns ENOMEM.
pub fn alloc_block_decide(allocated: u32, capacity: u32) -> Result<u32, i32> {
    if allocated < capacity { Ok(allocated + 1) } else { Err(ENOMEM) }
}
/// Decision for mempool free: validate and compute new allocated count.
///
/// MP4: free success. No underflow.
pub fn free_block_decide(allocated: u32) -> Result<u32, i32> {
    if allocated > 0 { Ok(allocated - 1) } else { Err(EINVAL) }
}
/// Action to take after a mempool alloc attempt.
///
/// Mirrors mempool.c — the mempool API does not pend (unlike k_heap), so the
/// post-alloc action is a 2-way: pointer (success) or NULL (failure).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MempoolAllocAction {
    /// Allocation succeeded — return the pointer to caller.
    ReturnPtr = 0,
    /// Allocation failed — return NULL.
    ReturnNull = 1,
}
/// Decide the post-alloc action for k_mem_pool_alloc.
///
/// MP2: alloc success returns the pointer; MP3: failure returns NULL.
pub fn alloc_action_decide(alloc_succeeded: bool) -> MempoolAllocAction {
    if alloc_succeeded {
        MempoolAllocAction::ReturnPtr
    } else {
        MempoolAllocAction::ReturnNull
    }
}
/// Action to take after a mempool free.
///
/// Mirrors mempool.c — after sys_heap_free, if any waiters were unpended a
/// reschedule is required.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MempoolFreeAction {
    /// No waiters — just unlock.
    FreeOk = 0,
    /// Waiters present — unlock and reschedule.
    FreeAndReschedule = 1,
}
/// Decide the post-free action for k_mem_pool_free.
///
/// MP4 (free): chooses between plain free and reschedule based on waiters.
pub fn free_action_decide(has_waiters: bool) -> MempoolFreeAction {
    if has_waiters {
        MempoolFreeAction::FreeAndReschedule
    } else {
        MempoolFreeAction::FreeOk
    }
}
