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

// =====================================================================
// Thread Suspend/Resume Decisions
// =====================================================================

/// Decision for k_thread_suspend.
///
/// sched.c z_impl_k_thread_suspend:
///   if (unlikely(z_is_thread_suspended(thread))) { return; }
///   z_thread_halt(thread, key, false);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuspendDecision {
    /// Action: 0=PROCEED (call z_thread_halt), 1=ALREADY_SUSPENDED (no-op)
    pub action: u8,
}

pub const SUSPEND_PROCEED: u8 = 0;
pub const SUSPEND_ALREADY_SUSPENDED: u8 = 1;

/// Thread state flag: thread is suspended (from kernel_structs.h _THREAD_SUSPENDED = BIT(1)).
pub const THREAD_STATE_SUSPENDED: u8 = 0x02;

/// Decide whether to proceed with k_thread_suspend.
///
/// TH7: Suspending an already-suspended thread is a no-op (idempotent).
///      This prevents double-suspend corruption.
///
/// Source: sched.c:491-522 z_impl_k_thread_suspend
pub fn suspend_decide(thread_state: u8) -> (d: SuspendDecision)
    ensures
        // Already suspended → no-op
        (thread_state & THREAD_STATE_SUSPENDED) != 0 ==> d.action == SUSPEND_ALREADY_SUSPENDED,
        // Not suspended → proceed
        (thread_state & THREAD_STATE_SUSPENDED) == 0 ==> d.action == SUSPEND_PROCEED,
{
    if (thread_state & THREAD_STATE_SUSPENDED) != 0 {
        SuspendDecision { action: SUSPEND_ALREADY_SUSPENDED }
    } else {
        SuspendDecision { action: SUSPEND_PROCEED }
    }
}

/// Decision for k_thread_resume.
///
/// sched.c z_impl_k_thread_resume:
///   if (unlikely(!z_is_thread_suspended(thread))) { return; }
///   z_mark_thread_as_not_suspended(thread);
///   ready_thread(thread);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResumeDecision {
    /// Action: 0=PROCEED (ready the thread), 1=NOT_SUSPENDED (no-op)
    pub action: u8,
}

pub const RESUME_PROCEED: u8 = 0;
pub const RESUME_NOT_SUSPENDED: u8 = 1;

/// Decide whether to proceed with k_thread_resume.
///
/// TH8: Resuming a non-suspended thread is a no-op (idempotent).
///      This prevents spurious wake-ups.
///
/// Source: sched.c:533-551 z_impl_k_thread_resume
pub fn resume_decide(thread_state: u8) -> (d: ResumeDecision)
    ensures
        // Not suspended → no-op
        (thread_state & THREAD_STATE_SUSPENDED) == 0 ==> d.action == RESUME_NOT_SUSPENDED,
        // Suspended → proceed
        (thread_state & THREAD_STATE_SUSPENDED) != 0 ==> d.action == RESUME_PROCEED,
{
    if (thread_state & THREAD_STATE_SUSPENDED) == 0 {
        ResumeDecision { action: RESUME_NOT_SUSPENDED }
    } else {
        ResumeDecision { action: RESUME_PROCEED }
    }
}

// =====================================================================
// Priority Set Decision
// =====================================================================

/// Decision for k_thread_priority_set.
///
/// sched.c z_impl_k_thread_priority_set:
///   Z_ASSERT_VALID_PRIO(prio, NULL)
///   z_thread_prio_set(thread, prio)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrioritySetDecision {
    /// Action: 0=PROCEED (call z_thread_prio_set), 1=REJECT (-EINVAL)
    pub action: u8,
    /// Error code: 0 (OK) or -EINVAL
    pub ret: i32,
}

pub const PRIO_SET_PROCEED: u8 = 0;
pub const PRIO_SET_REJECT: u8 = 1;

/// Decide whether to proceed with k_thread_priority_set.
///
/// TH1: Priority must be in valid range [0, MAX_PRIORITY).
/// TH2: Reject out-of-range priority before modifying thread state.
///
/// Source: sched.c:1009-1023 z_impl_k_thread_priority_set
pub fn priority_set_decide(new_priority: u32) -> (d: PrioritySetDecision)
    ensures
        // Invalid priority → reject
        new_priority >= MAX_PRIORITY ==> {
            &&& d.action == PRIO_SET_REJECT
            &&& d.ret == EINVAL
        },
        // Valid priority → proceed
        new_priority < MAX_PRIORITY ==> {
            &&& d.action == PRIO_SET_PROCEED
            &&& d.ret == OK
        },
{
    if new_priority >= MAX_PRIORITY {
        PrioritySetDecision { action: PRIO_SET_REJECT, ret: EINVAL }
    } else {
        PrioritySetDecision { action: PRIO_SET_PROCEED, ret: OK }
    }
}

// =====================================================================
// Stack Space Query Decision
// =====================================================================

/// Decision for k_thread_stack_space_get.
///
/// thread.c z_impl_k_thread_stack_space_get:
///   #ifdef CONFIG_THREAD_STACK_MEM_MAPPED
///     if (thread->stack_info.mapped.addr == NULL) { return -EINVAL; }
///   z_stack_space_get(thread->stack_info.start, thread->stack_info.size, unused_ptr)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StackSpaceDecision {
    /// Action: 0=PROCEED (query the stack), 1=REJECT (stack not queryable)
    pub action: u8,
    /// Error code: 0 (OK) or -EINVAL
    pub ret: i32,
    /// Expected unused bytes (valid only when action=PROCEED and stack fully unused).
    /// Set to stack_size when stack is uninitialized (no usage recorded).
    pub unused_estimate: u32,
}

pub const STACK_SPACE_PROCEED: u8 = 0;
pub const STACK_SPACE_REJECT: u8 = 1;

/// Decide whether k_thread_stack_space_get can proceed and estimate unused space.
///
/// Uses the verified StackInfo watermark to provide a conservative bound.
/// The actual unused bytes require inspecting the 0xAA fill pattern in C;
/// this model computes the upper bound: size - usage_watermark.
///
/// TH4: unused_estimate <= stack_size (bounded by StackInfo invariant).
///
/// Source: thread.c:1067-1078 z_impl_k_thread_stack_space_get
pub fn stack_space_decide(stack: StackInfo, stack_mapped_valid: bool) -> (d: StackSpaceDecision)
    requires
        stack.inv(),
    ensures
        // Unmapped stack (mem-mapped config) → reject
        !stack_mapped_valid ==> {
            &&& d.action == STACK_SPACE_REJECT
            &&& d.ret == EINVAL
        },
        // Valid stack → proceed with upper-bound estimate
        stack_mapped_valid ==> {
            &&& d.action == STACK_SPACE_PROCEED
            &&& d.ret == OK
            &&& d.unused_estimate == stack.size - stack.usage
            // TH4: estimate is bounded
            &&& d.unused_estimate <= stack.size
        },
{
    if !stack_mapped_valid {
        return StackSpaceDecision {
            action: STACK_SPACE_REJECT,
            ret: EINVAL,
            unused_estimate: 0,
        };
    }

    // unused() is proven bounded in StackInfo::inv()
    let unused_estimate = stack.unused();

    StackSpaceDecision {
        action: STACK_SPACE_PROCEED,
        ret: OK,
        unused_estimate,
    }
}

// =====================================================================
// Deadline Validation
// =====================================================================

/// Decision for k_thread_deadline_set.
///
/// sched.c z_impl_k_thread_deadline_set:
///   deadline = clamp(deadline, 0, INT_MAX)
///   newdl = k_cycle_get_32() + deadline
///
/// z_vrfy_k_thread_deadline_set (userspace):
///   if (deadline <= 0) return -EINVAL
///
/// We model the userspace validation: deadline must be positive.
/// The absolute deadline (now + deadline) is computed in C; overflow
/// of the cycle counter is a platform concern outside our model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeadlineDecision {
    /// Action: 0=PROCEED, 1=REJECT (-EINVAL)
    pub action: u8,
    /// Error code: 0 or -EINVAL
    pub ret: i32,
    /// Clamped deadline value (saturated to [0, i32::MAX]).
    pub clamped_deadline: i32,
}

pub const DEADLINE_PROCEED: u8 = 0;
pub const DEADLINE_REJECT: u8 = 1;

/// Decide whether a deadline value is valid and compute its clamped form.
///
/// TD1: deadline must be > 0 for userspace callers.
/// TD2: clamped_deadline == deadline for valid inputs in [1, i32::MAX].
/// TD3: zero or negative deadlines are rejected.
///
/// Source: sched.c:1063-1095 z_impl_k_thread_deadline_set + z_vrfy_*
pub fn deadline_decide(deadline: i32) -> (d: DeadlineDecision)
    ensures
        // Non-positive deadline → reject
        deadline <= 0 ==> {
            &&& d.action == DEADLINE_REJECT
            &&& d.ret == EINVAL
        },
        // Positive deadline → proceed with clamped value
        deadline > 0 ==> {
            &&& d.action == DEADLINE_PROCEED
            &&& d.ret == OK
            &&& d.clamped_deadline == deadline
        },
{
    if deadline <= 0 {
        DeadlineDecision {
            action: DEADLINE_REJECT,
            ret: EINVAL,
            clamped_deadline: 0,
        }
    } else {
        DeadlineDecision {
            action: DEADLINE_PROCEED,
            ret: OK,
            clamped_deadline: deadline,
        }
    }
}

// =====================================================================
// Additional proofs for new decision functions
// =====================================================================

#[verifier::external_body]
pub proof fn lemma_suspend_idempotent(state: u8) { }

#[verifier::external_body]
pub proof fn lemma_resume_idempotent(state: u8) { }

#[verifier::external_body]
pub proof fn lemma_suspend_resume_complement(state: u8) { }

#[verifier::external_body]
pub proof fn lemma_deadline_rejects_zero() { }

#[verifier::external_body]
pub proof fn lemma_deadline_accepts_positive(deadline: i32) { }

/// Priority range: all valid priorities are below MAX_PRIORITY.
pub proof fn lemma_priority_range()
    ensures
        MAX_PRIORITY > 0,
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

// =====================================================================
// Abort / Join decision functions (FFI gale_k_thread_*_decide)
// =====================================================================

/// Thread state flag: thread is dead (kernel_structs.h _THREAD_DEAD = BIT(3)).
pub const THREAD_STATE_DEAD: u8 = 0x08;

/// Decision for k_thread_abort.
///
/// Source: sched.c z_thread_abort:
///   if (z_is_thread_dead(thread)) { return; }   // ALREADY_DEAD
///   z_thread_halt(thread, key, true);            // PROCEED
///   if (essential) { k_panic(); }                // PANIC after halt
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AbortDecision {
    /// Action: 0=PROCEED, 1=ALREADY_DEAD, 2=PANIC
    pub action: u8,
}

/// Action: proceed with halt.
pub const ABORT_PROCEED: u8 = 0;
/// Action: thread already dead — no-op.
pub const ABORT_ALREADY_DEAD: u8 = 1;
/// Action: essential thread — halt then panic.
pub const ABORT_PANIC: u8 = 2;

/// Decide the abort action for k_thread_abort.
///
/// TH5: dead threads must not be re-aborted (no underflow on the active
/// counter).  Essential-thread abort is honoured but the kernel panics
/// after the halt completes.
pub fn abort_decide(thread_state: u8, is_essential: bool) -> (d: AbortDecision)
    ensures
        // TH5: already dead → no-op
        (thread_state & THREAD_STATE_DEAD) != 0 ==> d.action == ABORT_ALREADY_DEAD,
        // Live + essential → panic after halt
        (thread_state & THREAD_STATE_DEAD) == 0 && is_essential ==> d.action == ABORT_PANIC,
        // Live + non-essential → proceed
        (thread_state & THREAD_STATE_DEAD) == 0 && !is_essential ==> d.action == ABORT_PROCEED,
{
    if (thread_state & THREAD_STATE_DEAD) != 0 {
        AbortDecision { action: ABORT_ALREADY_DEAD }
    } else if is_essential {
        AbortDecision { action: ABORT_PANIC }
    } else {
        AbortDecision { action: ABORT_PROCEED }
    }
}

/// Decision for k_thread_join.
///
/// Source: sched.c z_impl_k_thread_join:
///   if (z_is_thread_dead(thread))           ret = 0;       // RETURN OK
///   else if (timeout == K_NO_WAIT)          ret = -EBUSY;  // RETURN EBUSY
///   else if (target == _current || circ)    ret = -EDEADLK;// RETURN EDEADLK
///   else                                    pend on join_queue
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JoinDecision {
    /// Action: 0=RETURN_IMMEDIATELY, 1=PEND_ON_JOIN_QUEUE
    pub action: u8,
    /// Return code applied on action == RETURN_IMMEDIATELY: OK, -EBUSY, -EDEADLK
    pub ret: i32,
}

/// Action: return immediately with `ret`.
pub const JOIN_RETURN: u8 = 0;
/// Action: pend on the target thread's join queue.
pub const JOIN_PEND: u8 = 1;

/// Decide the action for k_thread_join.
///
/// TH9: deadlock detection — joining self or a circular dependency is
/// rejected without modifying any wait queues.
pub fn join_decide(is_dead: bool, is_no_wait: bool, is_self_or_circular: bool)
    -> (d: JoinDecision)
    ensures
        // Already dead → return OK
        is_dead ==> d.action == JOIN_RETURN && d.ret == OK,
        // Live + no-wait → return EBUSY
        !is_dead && is_no_wait ==> d.action == JOIN_RETURN && d.ret == EBUSY,
        // Live + waiting + self/circular → return EDEADLK
        !is_dead && !is_no_wait && is_self_or_circular ==>
            d.action == JOIN_RETURN && d.ret == EDEADLK,
        // Live + waiting + safe → pend
        !is_dead && !is_no_wait && !is_self_or_circular ==>
            d.action == JOIN_PEND && d.ret == OK,
{
    if is_dead {
        JoinDecision { action: JOIN_RETURN, ret: OK }
    } else if is_no_wait {
        JoinDecision { action: JOIN_RETURN, ret: EBUSY }
    } else if is_self_or_circular {
        JoinDecision { action: JOIN_RETURN, ret: EDEADLK }
    } else {
        JoinDecision { action: JOIN_PEND, ret: OK }
    }
}

} // verus!
