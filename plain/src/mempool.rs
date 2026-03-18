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

#[doc = " Fixed-block memory pool model."]
#[doc = ""]
#[doc = " Models a pool of equal-sized blocks. Each block is either"]
#[doc = " allocated or free. We track the count, not individual blocks."]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemPool {
    #[doc = " Total number of blocks in the pool (immutable after init)."]
    pub capacity: u32,
    #[doc = " Number of blocks currently allocated."]
    pub allocated: u32,
    #[doc = " Size of each block in bytes (immutable after init)."]
    pub block_size: u32,
}
impl MemPool {
    #[doc = " Initialize a memory pool."]
    #[doc = ""]
    #[doc = " Returns EINVAL if capacity or block_size is 0."]
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
    #[doc = " Allocate one block from the pool."]
    #[doc = ""]
    #[doc = " MP2: success when allocated < capacity."]
    #[doc = " MP3: returns ENOMEM when full."]
    #[doc = " MP6: no overflow."]
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
    #[doc = " Allocate `count` blocks from the pool."]
    #[doc = ""]
    #[doc = " Returns OK if enough blocks are available, ENOMEM otherwise."]
    #[doc = " MP6: no overflow — checked addition."]
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
    #[doc = " Free one block back to the pool."]
    #[doc = ""]
    #[doc = " MP4: success when allocated > 0."]
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
    #[doc = " Free `count` blocks back to the pool."]
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
    #[doc = " Number of blocks currently allocated."]
    pub fn allocated_get(&self) -> u32 {
        self.allocated
    }
    #[doc = " Number of free (available) blocks."]
    #[doc = " MP5: free_blocks + allocated == capacity."]
    pub fn free_get(&self) -> u32 {
        #[allow(clippy::arithmetic_side_effects)]
        let r = self.capacity - self.allocated;
        r
    }
    #[doc = " Total pool capacity in blocks."]
    pub fn capacity_get(&self) -> u32 {
        self.capacity
    }
    #[doc = " Block size in bytes."]
    pub fn block_size_get(&self) -> u32 {
        self.block_size
    }
    #[doc = " Total pool size in bytes (capacity * block_size)."]
    #[doc = " MP6: overflow check."]
    pub fn total_size(&self) -> Option<u32> {
        let cap64: u64 = self.capacity as u64;
        let bs64: u64 = self.block_size as u64;
        #[allow(clippy::arithmetic_side_effects)]
        let total: u64 = cap64 * bs64;
        if total > u32::MAX as u64 {
            None
        } else {
            Some(total as u32)
        }
    }
    #[doc = " Check if the pool is full."]
    pub fn is_full(&self) -> bool {
        self.allocated == self.capacity
    }
    #[doc = " Check if the pool is empty (no blocks allocated)."]
    pub fn is_empty(&self) -> bool {
        self.allocated == 0
    }
}
