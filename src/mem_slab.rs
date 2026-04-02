//! Verified memory slab allocator for Zephyr RTOS.
//!
//! This is a formally verified port of zephyr/kernel/mem_slab.c.
//! All safety-critical properties are proven with Verus (SMT/Z3).
//!
//! This module models the **block allocation counter** of Zephyr's mem_slab
//! object.  Actual free-list pointer management and memory storage remain
//! in C — only the block count tracking crosses the FFI boundary.
//!
//! Source mapping:
//!   k_mem_slab_init      -> MemSlab::init      (mem_slab.c)
//!   k_mem_slab_alloc     -> MemSlab::alloc     (mem_slab.c, availability check + increment)
//!   k_mem_slab_free      -> MemSlab::free      (mem_slab.c, decrement)
//!   k_mem_slab_num_used_get  -> MemSlab::num_used_get
//!   k_mem_slab_num_free_get  -> MemSlab::num_free_get
//!
//! Omitted (not safety-relevant):
//!   - CONFIG_OBJ_CORE_MEM_SLAB — debug/tracing
//!   - CONFIG_USERSPACE (z_vrfy_*) — syscall marshaling
//!   - SYS_PORT_TRACING_* — instrumentation
//!   - k_mem_slab_runtime_stats_* — statistics
//!
//! ASIL-D verified properties:
//!   MS1: 0 <= num_used <= num_blocks (bounds invariant)
//!   MS2: num_blocks > 0 after init
//!   MS3: block_size > 0 after init
//!   MS4: alloc when num_used < num_blocks: num_used += 1, returns OK
//!   MS5: alloc when num_used == num_blocks: returns ENOMEM, unchanged
//!   MS6: free when num_used > 0: num_used -= 1, returns OK
//!   MS7: num_free + num_used == num_blocks (conservation)
//!   MS8: no arithmetic overflow in any operation

use vstd::prelude::*;
use crate::error::*;

verus! {

/// Memory slab allocator — block count model.
///
/// Corresponds to Zephyr's struct k_mem_slab {
///     uint32_t num_blocks;   // total blocks
///     size_t   block_size;   // bytes per block
///     uint32_t num_used;     // currently allocated
///     ...                    // free_list, buffer (in C)
/// };
///
/// We model the counter: num_used tracks allocated blocks.
/// The C shim manages the actual free-list pointers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemSlab {
    /// Total number of blocks (immutable after init).
    pub num_blocks: u32,
    /// Size of each block in bytes (immutable after init).
    pub block_size: u32,
    /// Number of currently allocated blocks.
    pub num_used: u32,
}

impl MemSlab {

    // ------------------------------------------------------------------
    // Specification predicates
    // ------------------------------------------------------------------

    /// Structural invariant — always maintained.
    pub open spec fn inv(&self) -> bool {
        self.num_blocks > 0
        && self.block_size > 0
        && self.num_used <= self.num_blocks
    }

    /// Slab is full (spec version for verification).
    pub open spec fn is_full_spec(&self) -> bool {
        self.num_used == self.num_blocks
    }

    /// Slab is empty / all blocks free (spec version for verification).
    pub open spec fn is_empty_spec(&self) -> bool {
        self.num_used == 0
    }

    // ------------------------------------------------------------------
    // Operations
    // ------------------------------------------------------------------

    /// Initialize a memory slab with given block_size and num_blocks.
    ///
    /// Returns EINVAL if block_size == 0 or num_blocks == 0.
    pub fn init(block_size: u32, num_blocks: u32) -> (result: Result<MemSlab, i32>)
        ensures
            match result {
                Ok(s) => s.inv()
                    && s.num_used == 0
                    && s.num_blocks == num_blocks
                    && s.block_size == block_size,
                Err(e) => e == EINVAL && (block_size == 0 || num_blocks == 0),
            }
    {
        if block_size == 0 || num_blocks == 0 {
            Err(EINVAL)
        } else {
            Ok(MemSlab { num_blocks, block_size, num_used: 0 })
        }
    }

    /// Allocate a block from the slab.
    ///
    /// Returns OK (num_used incremented) or ENOMEM (full, unchanged).
    pub fn alloc(&mut self) -> (rc: i32)
        requires old(self).inv(),
        ensures
            self.inv(),
            self.num_blocks == old(self).num_blocks,
            self.block_size == old(self).block_size,
            // MS4: not full -> num_used incremented
            old(self).num_used < old(self).num_blocks ==> {
                &&& rc == OK
                &&& self.num_used == old(self).num_used + 1
            },
            // MS5: full -> error, state unchanged
            old(self).num_used >= old(self).num_blocks ==> {
                &&& rc == ENOMEM
                &&& self.num_used == old(self).num_used
            },
    {
        if self.num_used >= self.num_blocks {
            ENOMEM
        } else {
            #[allow(clippy::arithmetic_side_effects)]
            {
                self.num_used = self.num_used + 1;
            }
            OK
        }
    }

    /// Free a block back to the slab.
    ///
    /// Returns OK (num_used decremented) or EINVAL (all blocks already free).
    pub fn free(&mut self) -> (rc: i32)
        requires old(self).inv(),
        ensures
            self.inv(),
            self.num_blocks == old(self).num_blocks,
            self.block_size == old(self).block_size,
            // MS6: not all free -> num_used decremented
            old(self).num_used > 0 ==> {
                &&& rc == OK
                &&& self.num_used == old(self).num_used - 1
            },
            // Free when all free -> error, state unchanged
            old(self).num_used == 0 ==> {
                &&& rc == EINVAL
                &&& self.num_used == old(self).num_used
            },
    {
        if self.num_used == 0 {
            EINVAL
        } else {
            #[allow(clippy::arithmetic_side_effects)]
            {
                self.num_used = self.num_used - 1;
            }
            OK
        }
    }

    /// Number of currently allocated blocks.
    pub fn num_used_get(&self) -> (r: u32)
        requires self.inv(),
        ensures r == self.num_used,
    {
        self.num_used
    }

    /// Number of free (available) blocks.
    pub fn num_free_get(&self) -> (r: u32)
        requires self.inv(),
        ensures r == self.num_blocks - self.num_used,
    {
        #[allow(clippy::arithmetic_side_effects)]
        let r = self.num_blocks - self.num_used;
        r
    }

    /// Total number of blocks.
    pub fn num_blocks_get(&self) -> (r: u32)
        requires self.inv(),
        ensures r == self.num_blocks,
    {
        self.num_blocks
    }

    /// Size of each block in bytes.
    pub fn block_size_get(&self) -> (r: u32)
        requires self.inv(),
        ensures r == self.block_size,
    {
        self.block_size
    }

    /// Check if slab is full (all blocks allocated).
    pub fn is_full(&self) -> (r: bool)
        requires self.inv(),
        ensures r == (self.num_used == self.num_blocks),
    {
        self.num_used == self.num_blocks
    }

    /// Check if slab is empty (no blocks allocated).
    pub fn is_empty(&self) -> (r: bool)
        requires self.inv(),
        ensures r == (self.num_used == 0),
    {
        self.num_used == 0
    }
}

// ======================================================================
// Compositional proofs
// ======================================================================

/// MS1/MS2/MS3: invariant is inductive across all operations.
/// The ensures clauses on alloc/free already prove this; this lemma
/// documents the property.
pub proof fn lemma_invariant_inductive()
    ensures
        // init establishes inv (from init's ensures)
        // alloc preserves inv (from alloc's ensures)
        // free preserves inv (from free's ensures)
        true,
{
}

/// MS4+MS6 roundtrip: alloc then free returns to original num_used.
pub proof fn lemma_alloc_free_roundtrip(num_used: u32, num_blocks: u32)
    requires
        num_blocks > 0,
        num_used < num_blocks,
    ensures ({
        // alloc: num_used -> num_used + 1
        let after_alloc = (num_used + 1) as u32;
        // free: num_used + 1 -> num_used
        let after_free = (after_alloc - 1) as u32;
        after_free == num_used
    })
{
}

/// MS7: free + used == num_blocks (conservation).
pub proof fn lemma_conservation(num_used: u32, num_blocks: u32)
    requires
        num_blocks > 0,
        num_used <= num_blocks,
    ensures
        (num_blocks - num_used) + num_used == num_blocks,
{
}

/// MS5: full slab rejects alloc.
pub proof fn lemma_full_rejects_alloc(num_used: u32, num_blocks: u32)
    requires
        num_blocks > 0,
        num_used == num_blocks,
    ensures
        num_used >= num_blocks,
{
}

/// Free when all blocks are free is rejected.
pub proof fn lemma_all_free_rejects_free(num_used: u32)
    requires
        num_used == 0u32,
    ensures
        num_used == 0u32,
{
}

/// Free then alloc returns to original num_used.
pub proof fn lemma_free_alloc_roundtrip(num_used: u32, num_blocks: u32)
    requires
        num_blocks > 0,
        num_used > 0,
        num_used <= num_blocks,
    ensures ({
        // free: num_used -> num_used - 1
        let after_free = (num_used - 1) as u32;
        // alloc: num_used - 1 -> num_used (since num_used - 1 < num_blocks)
        let after_alloc = (after_free + 1) as u32;
        after_alloc == num_used
    })
{
}

// =================================================================
// Lightweight decision functions — scalar-only, no MemSlab allocation.
// Used by FFI to delegate safety-critical logic to the verified model.
// =================================================================

/// Lightweight alloc decision — no MemSlab allocation.
#[derive(Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AllocDecision {
    /// Block available: num_used incremented.
    Alloc = 0,
    /// Slab full, willing to wait: pend current thread.
    Pend = 1,
    /// Slab full, no-wait: return immediately.
    NoMem = 2,
}

/// Result of an alloc decision with updated count.
#[derive(Debug)]
pub struct AllocDecideResult {
    pub decision: AllocDecision,
    pub new_num_used: u32,
}

/// Lightweight free decision — no MemSlab allocation.
#[derive(Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FreeDecision {
    /// Block returned to free list: num_used decremented.
    Free = 0,
    /// A waiter exists: give block directly to waiting thread (count unchanged).
    WakeThread = 1,
}

/// Result of a free decision with updated count.
#[derive(Debug)]
pub struct FreeDecideResult {
    pub decision: FreeDecision,
    pub new_num_used: u32,
}

/// Lightweight alloc decision — takes scalars, no MemSlab allocation.
///
/// Verified properties (MS4, MS5, MS1):
/// - num_used < num_blocks ==> Alloc (num_used + 1)
/// - num_used >= num_blocks && is_no_wait ==> NoMem
/// - num_used >= num_blocks && !is_no_wait ==> Pend
pub fn alloc_decide(
    num_used: u32,
    num_blocks: u32,
    is_no_wait: bool,
) -> (result: AllocDecideResult)
    requires
        true,
    ensures
        num_used < num_blocks ==> {
            &&& result.decision === AllocDecision::Alloc
            &&& result.new_num_used == num_used + 1
        },
        num_used >= num_blocks && is_no_wait ==> {
            &&& result.decision === AllocDecision::NoMem
            &&& result.new_num_used == num_used
        },
        num_used >= num_blocks && !is_no_wait ==> {
            &&& result.decision === AllocDecision::Pend
            &&& result.new_num_used == num_used
        },
{
    if num_used < num_blocks {
        AllocDecideResult {
            decision: AllocDecision::Alloc,
            new_num_used: num_used + 1,
        }
    } else if is_no_wait {
        AllocDecideResult {
            decision: AllocDecision::NoMem,
            new_num_used: num_used,
        }
    } else {
        AllocDecideResult {
            decision: AllocDecision::Pend,
            new_num_used: num_used,
        }
    }
}

/// Lightweight free decision — takes scalars, no MemSlab allocation.
///
/// Verified properties (MS6, MS1):
/// - has_waiter ==> WakeThread (count unchanged)
/// - !has_waiter && num_used > 0 ==> Free (count - 1)
/// - !has_waiter && num_used == 0 ==> Free (count unchanged, no-op)
pub fn free_decide(
    num_used: u32,
    has_waiter: bool,
) -> (result: FreeDecideResult)
    requires
        true,
    ensures
        has_waiter ==> {
            &&& result.decision === FreeDecision::WakeThread
            &&& result.new_num_used == num_used
        },
        !has_waiter && num_used > 0 ==> {
            &&& result.decision === FreeDecision::Free
            &&& result.new_num_used == (num_used - 1) as u32
        },
        !has_waiter && num_used == 0 ==> {
            &&& result.decision === FreeDecision::Free
            &&& result.new_num_used == 0
        },
{
    if has_waiter {
        FreeDecideResult {
            decision: FreeDecision::WakeThread,
            new_num_used: num_used,
        }
    } else if num_used > 0 {
        FreeDecideResult {
            decision: FreeDecision::Free,
            new_num_used: num_used - 1,
        }
    } else {
        FreeDecideResult {
            decision: FreeDecision::Free,
            new_num_used: 0,
        }
    }
}

} // verus!
