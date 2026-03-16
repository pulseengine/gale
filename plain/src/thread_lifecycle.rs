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
use crate::error::*;
use crate::priority::{MAX_PRIORITY, Priority};
/// Maximum number of threads tracked by the system.
/// Models CONFIG_MAX_THREAD_BYTES * 8 in Zephyr.
pub const MAX_THREADS: u32 = 256;
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Create a new stack info after thread creation.
    ///
    /// thread.c:495-497:
    ///   new_thread->stack_info.start = (uintptr_t)stack_buf_start;
    ///   new_thread->stack_info.size = stack_buf_size;
    pub fn init(base: u32, size: u32) -> Result<StackInfo, i32> {
        if size == 0 {
            Err(EINVAL)
        } else {
            Ok(StackInfo {
                base,
                size,
                usage: 0,
            })
        }
    }
    /// Record observed stack usage (watermark update).
    ///
    /// The watermark only increases — if the new measurement is lower
    /// than the current high-water-mark, it is ignored.
    ///
    /// TH4: usage <= size (bounded by the check).
    pub fn record_usage(&mut self, observed: u32) -> i32 {
        if observed > self.size {
            return EINVAL;
        }
        if observed > self.usage {
            self.usage = observed;
        }
        OK
    }
    /// Get remaining (unused) stack space.
    pub fn unused(&self) -> u32 {
        #[allow(clippy::arithmetic_side_effects)]
        let r = self.size - self.usage;
        r
    }
    /// Get the stack size.
    pub fn get_size(&self) -> u32 {
        self.size
    }
    /// Get the current usage watermark.
    pub fn get_usage(&self) -> u32 {
        self.usage
    }
}
/// Per-thread lifecycle information.
///
/// Combines the priority management from k_thread_priority_set/get
/// with the stack info tracking. This is separate from the existing
/// Thread struct (which models synchronization state transitions) and
/// from SchedThreadState (which models scheduler FSM transitions).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadInfo {
    /// Thread identifier.
    pub id: u32,
    /// Current priority.
    pub priority: u32,
    /// Stack information.
    pub stack: StackInfo,
}
impl ThreadInfo {
    /// Create a new thread info.
    ///
    /// Models the safety-critical parts of k_thread_create:
    ///   - Priority validation
    ///   - Stack info initialization
    pub fn new(
        id: u32,
        priority: u32,
        stack_base: u32,
        stack_size: u32,
    ) -> Result<ThreadInfo, i32> {
        if priority >= MAX_PRIORITY {
            return Err(EINVAL);
        }
        if stack_size == 0 {
            return Err(EINVAL);
        }
        Ok(ThreadInfo {
            id,
            priority,
            stack: StackInfo {
                base: stack_base,
                size: stack_size,
                usage: 0,
            },
        })
    }
    /// Get the current priority.
    ///
    /// thread.c:124-127:
    ///   return thread->base.prio;
    pub fn priority_get(&self) -> u32 {
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
    pub fn priority_set(&mut self, new_priority: u32) -> i32 {
        if new_priority >= MAX_PRIORITY {
            EINVAL
        } else {
            self.priority = new_priority;
            OK
        }
    }
}
/// System-wide thread resource tracker.
///
/// Counts active threads to detect resource leaks (threads created but
/// never exited/joined). This models the safety-critical counting aspect
/// of thread lifecycle management.
///
/// In Zephyr, thread objects are statically or dynamically allocated,
/// and failure to join/abort leaked threads can exhaust system resources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadTracker {
    /// Number of currently active threads.
    pub count: u32,
    /// Maximum threads ever active simultaneously (high-water-mark).
    pub peak: u32,
}
impl ThreadTracker {
    /// Create a new tracker with zero threads.
    pub fn new() -> Self {
        ThreadTracker { count: 0, peak: 0 }
    }
    /// Record a thread creation.
    ///
    /// TH6: returns error if at capacity (no overflow).
    pub fn create(&mut self) -> i32 {
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
    pub fn exit(&mut self) -> i32 {
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
    pub fn active_count(&self) -> u32 {
        self.count
    }
    /// Get the peak thread count.
    pub fn peak_count(&self) -> u32 {
        self.peak
    }
    /// Check if any threads are active.
    pub fn has_active(&self) -> bool {
        self.count > 0
    }
}
