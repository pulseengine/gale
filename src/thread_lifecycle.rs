//! Verified thread lifecycle management for Zephyr RTOS.
//!
//! This is a formally verified model of the safety-critical parts of
//! zephyr/kernel/thread.c. Only properties that affect system integrity
//! are modeled; naming, TLS, and arch-specific setup are omitted.
//!
//! This module covers:
//! 1. **Priority change validation** — priority stays in valid range
//! 2. **Stack info tracking** — stack base, size, usage watermark
//! 3. **Thread resource counting** — create/exit balance for leak detection
//!
//! Source mapping:
//!   k_thread_create            -> ThreadTracker::create      (thread.c:383-500)
//!   k_thread_priority_set      -> ThreadInfo::priority_set   (sched.c:1009-1023)
//!   k_thread_priority_get      -> ThreadInfo::priority_get   (thread.c:124-127)
//!   stack_info.start/size       -> StackInfo::init            (thread.c:495-497)
//!   stack usage watermark       -> StackInfo::record_usage    (thread.c:723-753)
//!
//! Omitted (not safety-relevant):
//!   - k_thread_name_set/get — string naming, no safety impact
//!   - Thread local storage — platform-specific, not modeled
//!   - CONFIG_OBJ_CORE_THREAD — debug/tracing
//!   - CONFIG_USERSPACE (z_vrfy_*) — syscall marshaling
//!   - SYS_PORT_TRACING_* — instrumentation
//!   - Shadow stack setup — arch-specific
//!   - Stack randomization — defense-in-depth, not safety-critical
//!
//! ASIL-D verified properties:
//!   TH1: priority always in valid range [0, MAX_PRIORITY)
//!   TH2: priority_set preserves thread invariant
//!   TH3: stack_size > 0 after create
//!   TH4: stack_usage <= stack_size (watermark bounded)
//!   TH5: thread count >= 0 (no underflow on exit)
//!   TH6: no overflow on thread count

use vstd::prelude::*;
use crate::error::*;
use crate::priority::{Priority, MAX_PRIORITY};

verus! {

/// Maximum number of threads tracked by the system.
/// Models CONFIG_MAX_THREAD_BYTES * 8 in Zephyr.
pub const MAX_THREADS: u32 = 256;

// =====================================================================
// Stack Info — stack base, size, and usage watermark
// =====================================================================

/// Stack information for a thread.
///
/// Models the safety-critical subset of Zephyr's k_thread.stack_info:
///   struct z_thread_stack_info {
///       uintptr_t start;   // usable stack start
///       size_t size;        // usable stack size
///       size_t delta;       // random offset (ignored here)
///       struct { size_t unused_threshold; } usage;
///   };
///
/// We track start (as an opaque id), size, and the high-water-mark
/// usage for runtime stack safety monitoring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StackInfo {
    /// Usable stack base address (opaque identifier).
    pub base: u32,
    /// Usable stack size in bytes.
    pub size: u32,
    /// High-water-mark: maximum observed stack usage in bytes.
    /// Corresponds to (stack_size - unused_threshold) tracking.
    pub usage: u32,
}

impl StackInfo {
    /// Structural invariant.
    /// TH3: size > 0
    /// TH4: usage <= size
    pub open spec fn inv(&self) -> bool {
        self.size > 0
        && self.usage <= self.size
    }

    /// Create a new stack info after thread creation.
    ///
    /// thread.c:495-497:
    ///   new_thread->stack_info.start = (uintptr_t)stack_buf_start;
    ///   new_thread->stack_info.size = stack_buf_size;
    pub fn init(base: u32, size: u32) -> (result: Result<StackInfo, i32>)
        ensures
            match result {
                Ok(si) => si.inv()
                    && si.base == base
                    && si.size == size
                    && si.usage == 0,
                Err(e) => e == EINVAL && size == 0,
            }
    {
        if size == 0 {
            Err(EINVAL)
        } else {
            Ok(StackInfo { base, size, usage: 0 })
        }
    }

    /// Record observed stack usage (watermark update).
    ///
    /// The watermark only increases — if the new measurement is lower
    /// than the current high-water-mark, it is ignored.
    ///
    /// TH4: usage <= size (bounded by the check).
    pub fn record_usage(&mut self, observed: u32) -> (rc: i32)
        requires
            old(self).inv(),
        ensures
            self.inv(),
            self.base == old(self).base,
            self.size == old(self).size,
            // Watermark only increases
            self.usage >= old(self).usage,
            // TH4: bounded
            self.usage <= self.size,
            // Success if valid, error if out of bounds
            observed <= old(self).size ==> rc == OK,
            observed > old(self).size ==> {
                &&& rc == EINVAL
                &&& self.usage == old(self).usage
            },
    {
        if observed > self.size {
            return EINVAL;
        }
        if observed > self.usage {
            self.usage = observed;
        }
        OK
    }

    /// Get remaining (unused) stack space.
    pub fn unused(&self) -> (result: u32)
        requires self.inv(),
        ensures result == self.size - self.usage,
    {
        #[allow(clippy::arithmetic_side_effects)]
        let r = self.size - self.usage;
        r
    }

    /// Get the stack size.
    pub fn get_size(&self) -> (result: u32)
        requires self.inv(),
        ensures result == self.size,
    {
        self.size
    }

    /// Get the current usage watermark.
    pub fn get_usage(&self) -> (result: u32)
        requires self.inv(),
        ensures result == self.usage,
    {
        self.usage
    }
}

// =====================================================================
// Thread Info — per-thread priority + stack tracking
// =====================================================================

/// Per-thread lifecycle information.
///
/// Combines the priority management from k_thread_priority_set/get
/// with the stack info tracking. This is separate from the existing
/// Thread struct (which models synchronization state transitions) and
/// from SchedThreadState (which models scheduler FSM transitions).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreadInfo {
    /// Thread identifier.
    pub id: u32,
    /// Current priority.
    pub priority: u32,
    /// Stack information.
    pub stack: StackInfo,
}

impl ThreadInfo {
    /// Structural invariant.
    /// TH1: priority in valid range
    pub open spec fn inv(&self) -> bool {
        self.priority < MAX_PRIORITY
        && self.stack.inv()
    }

    /// Create a new thread info.
    ///
    /// Models the safety-critical parts of k_thread_create:
    ///   - Priority validation
    ///   - Stack info initialization
    pub fn new(id: u32, priority: u32, stack_base: u32, stack_size: u32)
        -> (result: Result<ThreadInfo, i32>)
        ensures
            match result {
                Ok(ti) => ti.inv()
                    && ti.id == id
                    && ti.priority == priority
                    && ti.stack.base == stack_base
                    && ti.stack.size == stack_size
                    && ti.stack.usage == 0,
                Err(e) => e == EINVAL
                    && (priority >= MAX_PRIORITY || stack_size == 0),
            }
    {
        if priority >= MAX_PRIORITY {
            return Err(EINVAL);
        }
        if stack_size == 0 {
            return Err(EINVAL);
        }
        Ok(ThreadInfo {
            id,
            priority,
            stack: StackInfo { base: stack_base, size: stack_size, usage: 0 },
        })
    }

    /// Get the current priority.
    ///
    /// thread.c:124-127:
    ///   return thread->base.prio;
    pub fn priority_get(&self) -> (result: u32)
        requires self.inv(),
        ensures
            result == self.priority,
            result < MAX_PRIORITY,
    {
        self.priority
    }

    /// Set the thread priority.
    ///
    /// sched.c:1009-1023:
    ///   Z_ASSERT_VALID_PRIO(prio, NULL);
    ///   z_thread_prio_set(thread, prio);
    ///
    /// TH1: priority must be in valid range.
    /// TH2: invariant preserved after set.
    pub fn priority_set(&mut self, new_priority: u32) -> (rc: i32)
        requires
            old(self).inv(),
        ensures
            self.inv(),
            self.id == old(self).id,
            self.stack == old(self).stack,
            // TH1/TH2: valid priority -> updated, invariant preserved
            new_priority < MAX_PRIORITY ==> {
                &&& rc == OK
                &&& self.priority == new_priority
            },
            // Invalid priority -> error, state unchanged
            new_priority >= MAX_PRIORITY ==> {
                &&& rc == EINVAL
                &&& self.priority == old(self).priority
            },
    {
        if new_priority >= MAX_PRIORITY {
            EINVAL
        } else {
            self.priority = new_priority;
            OK
        }
    }
}

// =====================================================================
// Thread Tracker — system-wide thread count for resource leak detection
// =====================================================================

/// System-wide thread resource tracker.
///
/// Counts active threads to detect resource leaks (threads created but
/// never exited/joined). This models the safety-critical counting aspect
/// of thread lifecycle management.
///
/// In Zephyr, thread objects are statically or dynamically allocated,
/// and failure to join/abort leaked threads can exhaust system resources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreadTracker {
    /// Number of currently active threads.
    pub count: u32,
    /// Maximum threads ever active simultaneously (high-water-mark).
    pub peak: u32,
}

impl ThreadTracker {
    /// Structural invariant.
    /// TH5: count >= 0 (u32, trivially true)
    /// TH6: count <= MAX_THREADS (bounded)
    pub open spec fn inv(&self) -> bool {
        self.count <= MAX_THREADS
        && self.peak <= MAX_THREADS
        && self.count <= self.peak
    }

    /// Create a new tracker with zero threads.
    pub fn new() -> (result: Self)
        ensures
            result.inv(),
            result.count == 0,
            result.peak == 0,
    {
        ThreadTracker { count: 0, peak: 0 }
    }

    /// Record a thread creation.
    ///
    /// TH6: returns error if at capacity (no overflow).
    pub fn create(&mut self) -> (rc: i32)
        requires old(self).inv(),
        ensures
            self.inv(),
            // Success: count incremented
            old(self).count < MAX_THREADS ==> {
                &&& rc == OK
                &&& self.count == old(self).count + 1
                &&& self.peak >= old(self).peak
                &&& self.peak >= self.count
            },
            // At capacity: error, state unchanged
            old(self).count >= MAX_THREADS ==> {
                &&& rc == EAGAIN
                &&& self.count == old(self).count
                &&& self.peak == old(self).peak
            },
    {
        if self.count >= MAX_THREADS {
            return EAGAIN;
        }
        #[allow(clippy::arithmetic_side_effects)]
        {
            self.count = self.count + 1;
        }
        if self.count > self.peak {
            self.peak = self.count;
        }
        OK
    }

    /// Record a thread exit/abort.
    ///
    /// TH5: returns error if count is zero (no underflow).
    pub fn exit(&mut self) -> (rc: i32)
        requires old(self).inv(),
        ensures
            self.inv(),
            // Success: count decremented
            old(self).count > 0 ==> {
                &&& rc == OK
                &&& self.count == old(self).count - 1
                &&& self.peak == old(self).peak
            },
            // No threads: error, state unchanged
            old(self).count == 0 ==> {
                &&& rc == EINVAL
                &&& self.count == old(self).count
                &&& self.peak == old(self).peak
            },
    {
        if self.count == 0 {
            return EINVAL;
        }
        #[allow(clippy::arithmetic_side_effects)]
        {
            self.count = self.count - 1;
        }
        OK
    }

    /// Get the current thread count.
    pub fn active_count(&self) -> (result: u32)
        requires self.inv(),
        ensures result == self.count,
    {
        self.count
    }

    /// Get the peak thread count.
    pub fn peak_count(&self) -> (result: u32)
        requires self.inv(),
        ensures result == self.peak,
    {
        self.peak
    }

    /// Check if any threads are active.
    pub fn has_active(&self) -> (result: bool)
        requires self.inv(),
        ensures result == (self.count > 0),
    {
        self.count > 0
    }
}

// =====================================================================
// Compositional proofs
// =====================================================================

/// TH1/TH2: priority_set preserves invariant across all inputs.
pub proof fn lemma_priority_set_preserves_inv(priority: u32, new_priority: u32, stack: StackInfo)
    requires
        priority < MAX_PRIORITY,
        stack.inv(),
    ensures
        // Valid new priority -> invariant preserved
        new_priority < MAX_PRIORITY ==> new_priority < MAX_PRIORITY && stack.inv(),
        // Invalid new priority -> original priority preserved
        new_priority >= MAX_PRIORITY ==> priority < MAX_PRIORITY && stack.inv(),
{
}

/// TH3: stack size > 0 after successful init.
pub proof fn lemma_stack_size_positive(size: u32)
    requires size > 0,
    ensures size > 0,
{
}

/// TH4: usage watermark is always bounded by stack size.
pub proof fn lemma_usage_bounded(usage: u32, size: u32)
    requires
        size > 0,
        usage <= size,
    ensures
        usage <= size,
        size - usage >= 0,
{
}

/// TH5/TH6: create-exit roundtrip preserves count.
pub proof fn lemma_create_exit_roundtrip(count: u32, peak: u32)
    requires
        count < MAX_THREADS,
        peak <= MAX_THREADS,
        count <= peak,
    ensures ({
        let after_create = (count + 1) as u32;
        let after_exit = (after_create - 1) as u32;
        after_exit == count
    })
{
}

/// TH6: thread count never exceeds MAX_THREADS.
pub proof fn lemma_count_bounded(count: u32)
    requires count <= MAX_THREADS,
    ensures count <= MAX_THREADS,
{
}

/// Stack info conservation: usage + unused == size.
pub proof fn lemma_stack_conservation(usage: u32, size: u32)
    requires
        size > 0,
        usage <= size,
    ensures
        (size - usage) + usage == size,
{
}

/// Priority range: all valid priorities are below MAX_PRIORITY.
pub proof fn lemma_priority_range()
    ensures
        MAX_PRIORITY > 0,
        forall|p: u32| p < MAX_PRIORITY ==> p < MAX_PRIORITY,
{
}

/// Create then exit returns to original count.
pub proof fn lemma_exit_after_create(count: u32, peak: u32)
    requires
        count < MAX_THREADS,
        count <= peak,
        peak <= MAX_THREADS,
    ensures ({
        let new_count = (count + 1) as u32;
        // After exit: new_count - 1 == count
        let after_exit = (new_count - 1) as u32;
        after_exit == count
    })
{
}

/// Peak is monotonically non-decreasing: create can only increase peak.
pub proof fn lemma_peak_monotonic(count: u32, peak: u32)
    requires
        count < MAX_THREADS,
        count <= peak,
        peak <= MAX_THREADS,
    ensures ({
        let new_count = (count + 1) as u32;
        // If new_count > peak, new peak = new_count >= peak
        // If new_count <= peak, peak unchanged
        (new_count > peak ==> new_count >= peak) &&
        (new_count <= peak ==> peak >= peak)
    })
{
}

/// Usage watermark is monotonically non-decreasing.
pub proof fn lemma_watermark_monotonic(old_usage: u32, observed: u32, size: u32)
    requires
        size > 0,
        old_usage <= size,
        observed <= size,
    ensures
        // New usage is >= old usage
        (if observed > old_usage { observed } else { old_usage }) >= old_usage,
        // New usage is <= size
        (if observed > old_usage { observed } else { old_usage }) <= size,
{
}

} // verus!
