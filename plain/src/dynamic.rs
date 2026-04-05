//! Verified dynamic thread pool model for Zephyr RTOS.
//!
//! This is a formally verified model of Zephyr's dynamic thread pool
//! from kernel/dynamic.c. All safety-critical properties are proven
//! with Verus (SMT/Z3).
//!
//! This module models the **thread stack pool accounting** of Zephyr's
//! dynamic thread creation subsystem. Actual bitarray management, stack
//! allocation, and thread creation remain in C.
//!
//! Source mapping:
//!   z_thread_stack_alloc_pool -> DynamicPool::alloc     (dynamic.c:34-57)
//!   z_impl_k_thread_stack_free -> DynamicPool::free     (dynamic.c:116-158)
//!   CONFIG_DYNAMIC_THREAD_POOL_SIZE -> DynamicPool::max_threads
//!   CONFIG_DYNAMIC_THREAD_STACK_SIZE -> DynamicPool::stack_size
//!
//! Omitted (not safety-relevant):
//!   - z_thread_stack_alloc_dyn — heap-based allocation (uses k_malloc)
//!   - CONFIG_USERSPACE / k_object_* — kernel object management
//!   - dyn_cb / k_thread_foreach — thread enumeration for validation
//!   - SYS_BITARRAY_DEFINE_STATIC — static bitarray storage
//!
//! ASIL-D verified properties:
//!   DY1: 0 <= active <= max_threads (bounds invariant)
//!   DY2: alloc when active < max_threads: active += 1
//!   DY3: alloc when full: returns ENOMEM
//!   DY4: free when active > 0: active -= 1, no underflow
use crate::error::*;
/// Dynamic thread pool model.
///
/// Tracks the number of active (allocated) thread stacks from the
/// CONFIG_DYNAMIC_THREAD_POOL_SIZE pool. The bitarray that tracks
/// individual slot usage remains in C (sys_bitarray).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DynamicPool {
    /// Maximum threads in the pool (CONFIG_DYNAMIC_THREAD_POOL_SIZE).
    pub max_threads: u32,
    /// Number of currently allocated stacks.
    pub active: u32,
    /// Stack size for each thread (CONFIG_DYNAMIC_THREAD_STACK_SIZE).
    pub stack_size: u32,
}
impl DynamicPool {
    /// Initialize a dynamic thread pool.
    ///
    /// Returns EINVAL if max_threads or stack_size is 0.
    pub fn init(max_threads: u32, stack_size: u32) -> Result<DynamicPool, i32> {
        if max_threads == 0 || stack_size == 0 {
            Err(EINVAL)
        } else {
            Ok(DynamicPool {
                max_threads,
                active: 0,
                stack_size,
            })
        }
    }
    /// Allocate a thread stack from the pool.
    ///
    /// Models z_thread_stack_alloc_pool() (dynamic.c:34-57).
    ///
    /// DY2: success when active < max_threads.
    /// DY3: returns ENOMEM when full.
    pub fn alloc(&mut self) -> i32 {
        if self.active < self.max_threads {
            #[allow(clippy::arithmetic_side_effects)]
            {
                self.active = self.active + 1;
            }
            OK
        } else {
            ENOMEM
        }
    }
    /// Free a thread stack back to the pool.
    ///
    /// Models the pool portion of z_impl_k_thread_stack_free() (dynamic.c:116-158).
    ///
    /// DY4: success when active > 0, no underflow.
    pub fn free(&mut self) -> i32 {
        if self.active > 0 {
            #[allow(clippy::arithmetic_side_effects)]
            {
                self.active = self.active - 1;
            }
            OK
        } else {
            EINVAL
        }
    }
    /// Check if the requested stack size can be served by the pool.
    ///
    /// Models the size check in z_thread_stack_alloc_pool() (dynamic.c:40-43).
    pub fn can_serve(&self, requested_size: u32) -> bool {
        requested_size <= self.stack_size
    }
    /// Number of currently active (allocated) stacks.
    pub fn active_get(&self) -> u32 {
        self.active
    }
    /// Number of available (free) stacks.
    pub fn available_get(&self) -> u32 {
        #[allow(clippy::arithmetic_side_effects)]
        let r = self.max_threads - self.active;
        r
    }
    /// Maximum number of threads in the pool.
    pub fn max_threads_get(&self) -> u32 {
        self.max_threads
    }
    /// Stack size per thread.
    pub fn stack_size_get(&self) -> u32 {
        self.stack_size
    }
    /// Check if the pool is full.
    pub fn is_full(&self) -> bool {
        self.active == self.max_threads
    }
    /// Check if the pool is empty (no stacks allocated).
    pub fn is_empty(&self) -> bool {
        self.active == 0
    }
}
/// Decision for dynamic pool alloc: validate and compute new active count.
///
/// DY2: alloc success. DY3: full returns ENOMEM.
pub fn alloc_decide(active: u32, max_threads: u32) -> Result<u32, i32> {
    if active < max_threads { Ok(active + 1) } else { Err(ENOMEM) }
}
/// Decision for dynamic pool free: validate and compute new active count.
///
/// DY4: free success. No underflow.
pub fn free_decide(active: u32) -> Result<u32, i32> {
    if active > 0 { Ok(active - 1) } else { Err(EINVAL) }
}
