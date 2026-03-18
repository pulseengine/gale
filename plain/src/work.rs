//! Verified work queue model for Zephyr RTOS.
//!
//! This is a formally verified model of Zephyr's k_work kernel object
//! from kernel/work.c. All safety-critical properties are proven
//! with Verus (SMT/Z3).
//!
//! This module models the **work item state machine** of Zephyr's work queue.
//! Actual queue management, scheduling, and handler dispatch remain in C.
//! Only the state transitions and flag tracking cross the FFI boundary.
//!
//! Source mapping:
//!   k_work_init            -> WorkItem::init           (work.c:153-161)
//!   k_work_busy_get        -> WorkItem::busy_get       (work.c:169-177)
//!   k_work_submit_to_queue -> WorkItem::submit         (work.c:320-365)
//!   k_work_cancel          -> WorkItem::cancel         (work.c:501-520)
//!   finalize_cancel_locked -> WorkItem::finish_cancel  (work.c:128-151)
//!   (work queue thread)    -> WorkItem::start_running  (work.c work thread loop)
//!   (work queue thread)    -> WorkItem::finish_running (work.c work thread loop)
//!
//! Omitted (not safety-relevant):
//!   - k_work_delayable / k_work_schedule — timer-based delayed submission
//!   - k_work_flush / k_work_cancel_sync — blocking synchronization
//!   - k_work_queue_start / k_work_queue_drain — queue lifecycle
//!   - SYS_PORT_TRACING_* — instrumentation
//!   - CONFIG_KERNEL_COHERENCE — cache coherency
//!   - pending_cancels list — multi-waiter synchronization detail
//!   - handler function pointer — application callback
//!
//! ASIL-D verified properties:
//!   WK1: init produces IDLE state (no flags set)
//!   WK2: submit from IDLE sets QUEUED flag
//!   WK3: submit while CANCELING returns EBUSY
//!   WK4: submit while already QUEUED is idempotent (returns 0)
//!   WK5: cancel clears QUEUED and sets CANCELING if RUNNING
//!   WK6: state flags are mutually consistent (QUEUED+RUNNING valid, not IDLE+RUNNING)
use crate::error::*;
/// Work item flag bits — matches kernel.h K_WORK_*_BIT.
pub const RUNNING_BIT: u8 = 0;
pub const CANCELING_BIT: u8 = 1;
pub const QUEUED_BIT: u8 = 2;
pub const FLUSHING_BIT: u8 = 4;
/// Work item flag masks.
pub const FLAG_RUNNING: u8 = 1;
pub const FLAG_CANCELING: u8 = 2;
pub const FLAG_QUEUED: u8 = 4;
pub const FLAG_FLUSHING: u8 = 16;
/// Busy mask: RUNNING | CANCELING | QUEUED
pub const BUSY_MASK: u8 = 7;
/// Work item state machine model.
///
/// Corresponds to Zephyr's struct k_work {
///     sys_snode_t node;         // queue linkage (not modeled)
///     k_work_handler_t handler; // callback (not modeled)
///     struct k_work_q *queue;   // owning queue (not modeled)
///     uint32_t flags;           // state flags (modeled as u8)
/// };
///
/// We model the flags field to track work item lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkItem {
    /// State flags (RUNNING, CANCELING, QUEUED, FLUSHING).
    pub flags: u8,
}
impl WorkItem {
    /// Initialize a work item.
    ///
    /// Corresponds to k_work_init() (work.c:153-161).
    /// WK1: produces IDLE state.
    pub fn init() -> WorkItem {
        let w = WorkItem { flags: 0 };
        w
    }
    /// Get the busy status flags.
    ///
    /// Corresponds to k_work_busy_get() (work.c:169-177).
    /// Returns the RUNNING | CANCELING | QUEUED mask.
    pub fn busy_get(&self) -> u8 {
        #[allow(clippy::arithmetic_side_effects)]
        let r = self.flags & BUSY_MASK;
        r
    }
    /// Check if the work item is idle (not busy).
    pub fn is_idle(&self) -> bool {
        (self.flags & BUSY_MASK) == 0
    }
    /// Check if the work item is queued.
    pub fn is_queued(&self) -> bool {
        (self.flags & FLAG_QUEUED) != 0
    }
    /// Check if the work item is running.
    pub fn is_running(&self) -> bool {
        (self.flags & FLAG_RUNNING) != 0
    }
    /// Check if the work item is being canceled.
    pub fn is_canceling(&self) -> bool {
        (self.flags & FLAG_CANCELING) != 0
    }
    /// Submit a work item to a queue.
    ///
    /// Models submit_to_queue_locked() (work.c:320-365).
    ///
    /// Returns:
    ///   1  — newly queued
    ///   2  — was running, re-queued
    ///   0  — already queued (no-op)
    ///   EBUSY — canceling, rejected
    ///
    /// WK2: IDLE -> sets QUEUED flag
    /// WK3: CANCELING -> returns EBUSY
    /// WK4: already QUEUED -> returns 0 (idempotent)
    pub fn submit(&mut self) -> i32 {
        if (self.flags & FLAG_CANCELING) != 0 {
            return EBUSY;
        }
        if (self.flags & FLAG_QUEUED) != 0 {
            return 0;
        }
        let was_running = (self.flags & FLAG_RUNNING) != 0;
        let old_flags = self.flags;
        #[allow(clippy::arithmetic_side_effects)]
        {
            self.flags = self.flags | FLAG_QUEUED;
        }
        if was_running { 2 } else { 1 }
    }
    /// Begin execution of a work item (called by work queue thread).
    ///
    /// Dequeues the item and marks it as running.
    /// Precondition: item must be queued.
    pub fn start_running(&mut self) {
        let old_flags = self.flags;
        #[allow(clippy::arithmetic_side_effects)]
        {
            self.flags = (self.flags & !FLAG_QUEUED) | FLAG_RUNNING;
        }
    }
    /// Complete execution of a work item (called by work queue thread).
    ///
    /// Clears the RUNNING flag.
    /// Precondition: item must be running.
    pub fn finish_running(&mut self) {
        let old_flags = self.flags;
        #[allow(clippy::arithmetic_side_effects)]
        {
            self.flags = self.flags & !FLAG_RUNNING;
        }
    }
    /// Cancel a work item (async portion).
    ///
    /// Models cancel_async_locked() (work.c:501-520).
    /// Clears QUEUED if set. If still busy (RUNNING), sets CANCELING.
    ///
    /// WK5: clears QUEUED, sets CANCELING if still running.
    ///
    /// Returns the busy flags after cancellation attempt.
    pub fn cancel(&mut self) -> u8 {
        let old_flags = self.flags;
        #[allow(clippy::arithmetic_side_effects)]
        {
            self.flags = self.flags & !FLAG_QUEUED;
        }
        let busy = self.flags & BUSY_MASK;
        if busy != 0 {
            let mid_flags = self.flags;
            #[allow(clippy::arithmetic_side_effects)]
            {
                self.flags = self.flags | FLAG_CANCELING;
            }
        }
        self.flags & BUSY_MASK
    }
    /// Complete cancellation of a work item.
    ///
    /// Models finalize_cancel_locked() (work.c:128-151).
    /// Clears the CANCELING flag. Called when the running handler completes.
    pub fn finish_cancel(&mut self) {
        let old_flags = self.flags;
        #[allow(clippy::arithmetic_side_effects)]
        {
            self.flags = self.flags & !FLAG_CANCELING;
        }
    }
}
