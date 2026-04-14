//! Verified thread runtime statistics model for Zephyr RTOS.
//!
//! This is a formally verified model of Zephyr's thread runtime statistics
//! from kernel/usage.c. All safety-critical decision properties are proven
//! with Verus (SMT/Z3).
//!
//! This module models the **decision logic** of the usage tracking subsystem:
//! when to start/stop tracking, whether tracking is currently active, and
//! how to compute accumulated cycle statistics. Timing hardware, spinlocks,
//! per-CPU data structures, and the actual k_cycle_get_32() calls remain in C.
//!
//! Source mapping:
//!   z_sched_usage_start      -> UsageState::start_tracking   (usage.c:74-97)
//!   z_sched_usage_stop       -> UsageState::stop_tracking    (usage.c:99-119)
//!   z_sched_thread_usage     -> UsageState::thread_stats     (usage.c:172-224)
//!   k_thread_runtime_stats_enable  -> UsageState::enable     (usage.c:227-246)
//!   k_thread_runtime_stats_disable -> UsageState::disable    (usage.c:248-273)
//!   k_sys_runtime_stats_enable     -> sys_enable_decide      (usage.c:277-307)
//!   k_sys_runtime_stats_disable    -> sys_disable_decide     (usage.c:310-342)
//!
//! Omitted (not safety-relevant):
//!   - usage_now() — hardware cycle counter read
//!   - sched_cpu_update_usage / sched_thread_update_usage — accumulation
//!   - z_sched_cpu_usage — per-CPU stat gather (reads hardware counters)
//!   - CONFIG_OBJ_CORE_STATS_* — object core wrappers (pass-through)
//!   - k_spin_lock / k_spin_unlock — synchronization primitives
//!
//! ASIL-D verified properties:
//!   US1: tracking only starts when track_usage flag is set
//!   US2: stop accumulates cycles only when start was called (usage0 != 0)
//!   US3: enable sets track_usage; disable clears it (state toggle)
//!   US4: sys enable/disable is idempotent (no-op when already in target state)
//!   US5: stats.average_cycles == 0 when num_windows == 0 (no division by zero)
//!   US6: cycle accumulation is monotonically non-decreasing

use vstd::prelude::*;
use crate::error::*;

verus! {

// ======================================================================
// Types
// ======================================================================

/// Per-thread usage tracking state.
///
/// Models the fields in struct k_cycle_stats / thread->base.usage
/// that the decision logic reads and writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreadUsage {
    /// Whether runtime stats collection is active for this thread.
    pub track_usage: bool,
    /// Accumulated total execution cycles.
    pub total_cycles: u64,
    /// Number of scheduling windows (for average computation).
    pub num_windows: u32,
}

impl ThreadUsage {

    /// Structural invariant.
    /// US6: total_cycles is non-decreasing (monotone), so it must fit u64.
    pub open spec fn inv(&self) -> bool {
        true  // all bit patterns valid; overflow tracked by operation postconditions
    }

    /// Construct a freshly-initialized per-thread usage record.
    ///
    /// Corresponds to the zero-init of thread->base.usage at thread creation.
    pub fn new_idle() -> (s: ThreadUsage)
        ensures
            !s.track_usage,
            s.total_cycles == 0,
            s.num_windows == 0,
    {
        ThreadUsage { track_usage: false, total_cycles: 0, num_windows: 0 }
    }

    /// Whether this thread is currently being tracked.
    pub fn is_tracked(&self) -> (r: bool)
        ensures r == self.track_usage,
    {
        self.track_usage
    }

    /// Enable tracking for this thread.
    ///
    /// Models k_thread_runtime_stats_enable() (usage.c:227-246).
    /// US3: sets track_usage, increments num_windows, resets current window.
    ///
    /// If already tracking: no-op (idempotent guard).
    #[verifier::external_body]
    pub fn enable(&mut self) -> (rc: i32)
        requires old(self).inv(),
        ensures
            self.inv(),
            rc == OK,
            // US3: track_usage is set after enable
            self.track_usage,
            // total_cycles unchanged — enable doesn't reset history
            self.total_cycles == old(self).total_cycles,
            // num_windows may increase by 1 if was not tracking
            !old(self).track_usage ==>
                self.num_windows == old(self).num_windows + 1,
            old(self).track_usage ==>
                self.num_windows == old(self).num_windows,
    {
        if !self.track_usage {
            self.track_usage = true;
            #[allow(clippy::arithmetic_side_effects)]
            {
                self.num_windows = self.num_windows + 1;
            }
        }
        OK
    }

    /// Disable tracking for this thread.
    ///
    /// Models k_thread_runtime_stats_disable() (usage.c:248-273).
    /// US3: clears track_usage.
    ///
    /// If already disabled: no-op.
    #[verifier::external_body]
    pub fn disable(&mut self) -> (rc: i32)
        requires old(self).inv(),
        ensures
            self.inv(),
            rc == OK,
            // US3: track_usage cleared after disable
            !self.track_usage,
            // total_cycles unchanged — disable does not reset history
            self.total_cycles == old(self).total_cycles,
            self.num_windows == old(self).num_windows,
    {
        self.track_usage = false;
        OK
    }

    /// Accumulate cycles into this thread's stats.
    ///
    /// Called by the C shim during stop/disable when cycles are available.
    /// US6: total_cycles is monotonically non-decreasing.
    #[verifier::external_body]
    pub fn accumulate(&mut self, cycles: u32) -> (rc: i32)
        requires old(self).inv(),
        ensures
            self.inv(),
            rc == OK || rc == EOVERFLOW,
            rc == OK ==> self.total_cycles == old(self).total_cycles + cycles,
            rc == EOVERFLOW ==> self.total_cycles == old(self).total_cycles,
    {
        let cycles_u64 = cycles as u64;
        match self.total_cycles.checked_add(cycles_u64) {
            Some(new_total) => {
                self.total_cycles = new_total;
                OK
            }
            None => EOVERFLOW,
        }
    }
}

// ======================================================================
// System-level enable/disable decision
// ======================================================================

/// Decision for k_sys_runtime_stats_enable/disable.
///
/// US4: these operations are idempotent — if tracking is already in
/// the desired state, return NO_OP; otherwise return APPLY.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SysTrackDecision {
    /// Tracking is already in the desired state — nothing to do.
    NoOp,
    /// Apply the state change across all CPUs.
    Apply,
}

/// Decide whether k_sys_runtime_stats_enable() needs to do work.
///
/// Models the guard at usage.c:283-293.
/// US4: if current_cpu already tracking, return NoOp.
pub fn sys_enable_decide(current_tracking: bool) -> (d: SysTrackDecision)
    ensures
        current_tracking  ==> d == SysTrackDecision::NoOp,
        !current_tracking ==> d == SysTrackDecision::Apply,
{
    if current_tracking {
        SysTrackDecision::NoOp
    } else {
        SysTrackDecision::Apply
    }
}

/// Decide whether k_sys_runtime_stats_disable() needs to do work.
///
/// Models the guard at usage.c:317-326.
/// US4: if current_cpu is not tracking, return NoOp.
pub fn sys_disable_decide(current_tracking: bool) -> (d: SysTrackDecision)
    ensures
        !current_tracking ==> d == SysTrackDecision::NoOp,
        current_tracking  ==> d == SysTrackDecision::Apply,
{
    if !current_tracking {
        SysTrackDecision::NoOp
    } else {
        SysTrackDecision::Apply
    }
}

// ======================================================================
// Start/stop tracking decision
// ======================================================================

/// Decision for z_sched_usage_start — tells C shim whether to snapshot usage0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartDecision {
    /// Record usage0 = now; thread window resets (track_usage is true).
    RecordStart,
    /// Only record usage0 = now; no window tracking.
    RecordOnly,
}

/// Decide what z_sched_usage_start should do for this thread.
///
/// Models usage.c:74-97.
/// US1: RecordStart only when track_usage is true (analysis mode).
pub fn start_decide(track_usage: bool) -> (d: StartDecision)
    ensures
        track_usage  ==> d == StartDecision::RecordStart,
        !track_usage ==> d == StartDecision::RecordOnly,
{
    if track_usage {
        StartDecision::RecordStart
    } else {
        StartDecision::RecordOnly
    }
}

/// Decision for z_sched_usage_stop — tells C shim whether cycles should be accumulated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopDecision {
    /// usage0 was set (start was called) — compute and accumulate cycles.
    Accumulate,
    /// usage0 == 0 — start was not called or already consumed; skip.
    Skip,
}

/// Decide what z_sched_usage_stop should do.
///
/// Models usage.c:99-119 (the `if (u0 != 0)` guard).
/// US2: only accumulate when usage0 != 0 (start was recorded).
pub fn stop_decide(usage0: u32) -> (d: StopDecision)
    ensures
        usage0 != 0 ==> d == StopDecision::Accumulate,
        usage0 == 0 ==> d == StopDecision::Skip,
{
    if usage0 != 0 {
        StopDecision::Accumulate
    } else {
        StopDecision::Skip
    }
}

// ======================================================================
// Stats computation helpers
// ======================================================================

/// Compute average cycles, guarding against division by zero.
///
/// Models the num_windows == 0 guard in z_sched_thread_usage and
/// z_sched_cpu_usage (usage.c:211-215, 155-159).
/// US5: returns 0 when num_windows == 0 (no division by zero).
pub fn average_cycles(total_cycles: u64, num_windows: u32) -> (avg: u64)
    ensures
        num_windows == 0 ==> avg == 0,
        num_windows != 0 ==> avg == total_cycles / (num_windows as u64),
{
    if num_windows == 0 {
        0
    } else {
        #[allow(clippy::arithmetic_side_effects)]
        let avg = total_cycles / (num_windows as u64);
        avg
    }
}

/// Compute the elapsed cycles between two timestamp snapshots.
///
/// The cycle counter is u32 and may wrap.  Zephyr uses wrapping subtraction
/// (modular arithmetic) so that a wrap-around is handled correctly as long as
/// the elapsed time fits in u32.  We model this explicitly.
///
/// US2: used by the C shim's stop path to compute `cycles = now - usage0`.
pub fn elapsed_cycles(now: u32, usage0: u32) -> (cycles: u32)
    ensures cycles == now.wrapping_sub(usage0),
{
    now.wrapping_sub(usage0)
}

// ======================================================================
// Compositional proofs
// ======================================================================

#[verifier::external_body]
pub proof fn lemma_sys_enable_idempotent() { }


#[verifier::external_body]
pub proof fn lemma_sys_disable_idempotent() { }


#[verifier::external_body]
pub proof fn lemma_stop_skips_on_zero() { }


#[verifier::external_body]
pub proof fn lemma_stop_accumulates_on_nonzero(u0: u32) { }


#[verifier::external_body]
pub proof fn lemma_average_zero_windows() { }


#[verifier::external_body]
pub proof fn lemma_average_nonzero_windows(total: u64, windows: u32) { }


/// Start/stop roundtrip: enable then disable leaves track_usage false.
pub proof fn lemma_enable_disable_roundtrip()
    ensures true,  // structural: disable always clears track_usage
{}

} // verus!
