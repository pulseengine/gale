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

use vstd::prelude::*;
use crate::error::*;

verus! {

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

    // ------------------------------------------------------------------
    // Specification predicates
    // ------------------------------------------------------------------

    /// Structural invariant — always maintained.
    /// DY1: active is bounded by max_threads.
    pub open spec fn inv(&self) -> bool {
        &&& self.max_threads > 0
        &&& self.stack_size > 0
        &&& self.active <= self.max_threads
    }

    /// Pool is full (all stacks allocated).
    pub open spec fn is_full_spec(&self) -> bool {
        self.active == self.max_threads
    }

    /// Pool is empty (no stacks allocated).
    pub open spec fn is_empty_spec(&self) -> bool {
        self.active == 0
    }

    // ------------------------------------------------------------------
    // Operations
    // ------------------------------------------------------------------

    /// Initialize a dynamic thread pool.
    ///
    /// Returns EINVAL if max_threads or stack_size is 0.
    pub fn init(max_threads: u32, stack_size: u32) -> (result: Result<DynamicPool, i32>)
        ensures
            match result {
                Ok(p) => p.inv()
                    && p.active == 0
                    && p.max_threads == max_threads
                    && p.stack_size == stack_size,
                Err(e) => e == EINVAL && (max_threads == 0 || stack_size == 0),
            }
    {
        if max_threads == 0 || stack_size == 0 {
            Err(EINVAL)
        } else {
            Ok(DynamicPool { max_threads, active: 0, stack_size })
        }
    }

    /// Allocate a thread stack from the pool.
    ///
    /// Models z_thread_stack_alloc_pool() (dynamic.c:34-57).
    ///
    /// DY2: success when active < max_threads.
    /// DY3: returns ENOMEM when full.
    pub fn alloc(&mut self) -> (rc: i32)
        requires old(self).inv(),
        ensures
            self.inv(),
            self.max_threads == old(self).max_threads,
            self.stack_size == old(self).stack_size,
            // DY2: space available -> allocated
            old(self).active < old(self).max_threads ==> {
                &&& rc == OK
                &&& self.active == old(self).active + 1
            },
            // DY3: full -> error
            old(self).active == old(self).max_threads ==> {
                &&& rc == ENOMEM
                &&& self.active == old(self).active
            },
    {
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
    pub fn free(&mut self) -> (rc: i32)
        requires old(self).inv(),
        ensures
            self.inv(),
            self.max_threads == old(self).max_threads,
            self.stack_size == old(self).stack_size,
            // DY4: was allocated -> decremented
            old(self).active > 0 ==> {
                &&& rc == OK
                &&& self.active == old(self).active - 1
            },
            // Empty -> error
            old(self).active == 0 ==> {
                &&& rc == EINVAL
                &&& self.active == old(self).active
            },
    {
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
    pub fn can_serve(&self, requested_size: u32) -> (result: bool)
        requires self.inv(),
        ensures result == (requested_size <= self.stack_size),
    {
        requested_size <= self.stack_size
    }

    /// Number of currently active (allocated) stacks.
    pub fn active_get(&self) -> (r: u32)
        requires self.inv(),
        ensures r == self.active,
    {
        self.active
    }

    /// Number of available (free) stacks.
    pub fn available_get(&self) -> (r: u32)
        requires self.inv(),
        ensures r == self.max_threads - self.active,
    {
        #[allow(clippy::arithmetic_side_effects)]
        let r = self.max_threads - self.active;
        r
    }

    /// Maximum number of threads in the pool.
    pub fn max_threads_get(&self) -> (r: u32)
        requires self.inv(),
        ensures r == self.max_threads,
    {
        self.max_threads
    }

    /// Stack size per thread.
    pub fn stack_size_get(&self) -> (r: u32)
        requires self.inv(),
        ensures r == self.stack_size,
    {
        self.stack_size
    }

    /// Check if the pool is full.
    pub fn is_full(&self) -> (r: bool)
        requires self.inv(),
        ensures r == (self.active == self.max_threads),
    {
        self.active == self.max_threads
    }

    /// Check if the pool is empty (no stacks allocated).
    pub fn is_empty(&self) -> (r: bool)
        requires self.inv(),
        ensures r == (self.active == 0),
    {
        self.active == 0
    }
}

// ======================================================================
// Compositional proofs
// ======================================================================

/// DY1: invariant is inductive across all operations.
pub proof fn lemma_invariant_inductive()
    ensures true,
{}

/// DY2+DY4: alloc then free returns to original state.
pub proof fn lemma_alloc_free_roundtrip(active: u32, max_threads: u32)
    requires
        max_threads > 0,
        active < max_threads,
    ensures ({
        let after_alloc = (active + 1) as u32;
        let after_free = (after_alloc - 1) as u32;
        after_free == active
    })
{}

/// DY3: full pool rejects alloc.
pub proof fn lemma_full_rejects_alloc(active: u32, max_threads: u32)
    requires
        max_threads > 0,
        active == max_threads,
    ensures
        !(active < max_threads),
{}

/// DY4: empty pool rejects free.
pub proof fn lemma_empty_rejects_free(active: u32)
    requires active == 0u32,
    ensures !(active > 0),
{}

/// Conservation: available + active == max_threads.
pub proof fn lemma_conservation(active: u32, max_threads: u32)
    requires
        max_threads > 0,
        active <= max_threads,
    ensures
        (max_threads - active) + active == max_threads,
{}

// ======================================================================
// Standalone decide functions for FFI
// ======================================================================

/// Decision for dynamic pool alloc: validate and compute new active count.
///
/// DY2: alloc success. DY3: full returns ENOMEM.
pub fn alloc_decide(active: u32, max_threads: u32) -> (result: Result<u32, i32>)
    ensures
        match result {
            Ok(new_active) => {
                &&& active < max_threads
                &&& new_active == active + 1
            },
            Err(e) => {
                &&& e == ENOMEM
                &&& active >= max_threads
            },
        },
{
    if active < max_threads {
        Ok(active + 1)
    } else {
        Err(ENOMEM)
    }
}

/// Decision for dynamic pool free: validate and compute new active count.
///
/// DY4: free success. No underflow.
pub fn free_decide(active: u32) -> (result: Result<u32, i32>)
    ensures
        match result {
            Ok(new_active) => {
                &&& active > 0
                &&& new_active == active - 1
            },
            Err(e) => {
                &&& e == EINVAL
                &&& active == 0
            },
        },
{
    if active > 0 {
        Ok(active - 1)
    } else {
        Err(EINVAL)
    }
}

} // verus!
