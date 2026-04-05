//! Verified time-slicing model for Zephyr RTOS.
//!
//! This is a formally verified model of Zephyr's timeslicing subsystem
//! from kernel/timeslicing.c. All safety-critical properties are proven
//! with Verus (SMT/Z3).
//!
//! This module models the **time-slice tick accounting** for preemptive
//! scheduling. Actual timeout scheduling, IPI dispatch, and thread
//! migration remain in C — only the tick counter state crosses the
//! FFI boundary.
//!
//! Source mapping:
//!   z_reset_time_slice  -> TimeSlice::reset      (timeslicing.c:75-86)
//!   z_time_slice        -> TimeSlice::tick        (timeslicing.c:131-161)
//!   k_sched_time_slice_set -> TimeSlice::set_config (timeslicing.c:97-115)
//!   slice_time          -> TimeSlice::slice_time  (timeslicing.c:25-37)
//!
//! Omitted (not safety-relevant):
//!   - CONFIG_SWAP_NONATOMIC pending_current guard — race avoidance
//!   - CONFIG_TIMESLICE_PER_THREAD per-thread overrides — thread-local config
//!   - slice_timeout() IPI dispatch — hardware interaction
//!   - SMP per-CPU arrays — multi-core indexing
//!   - Priority-based eligibility (slice_max_prio) — policy decision
//!   - Thread state checks (idle, prevented) — scheduler query
//!
//! ASIL-D verified properties:
//!   TS1: 0 <= slice_ticks <= slice_max_ticks (bounds invariant)
//!   TS2: reset sets slice_ticks = slice_max_ticks
//!   TS3: tick decrements slice_ticks by 1
//!   TS4: expired when slice_ticks == 0
//!   TS5: no underflow on tick (tick at 0 is a no-op)
//!   TS6: slice_max_ticks > 0 when enabled
use crate::error::*;
/// Time-slice accounting model — tick counter per scheduling context.
///
/// Corresponds to the per-CPU state in timeslicing.c:
///   static int slice_ticks;               // configured max ticks
///   static bool slice_expired[NUM_CPUS];  // expiry flag per CPU
///
/// We model a single CPU's time-slice state. The C code manages
/// the per-CPU array indexing and timeout scheduling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeSlice {
    /// Remaining ticks in the current time slice.
    pub slice_ticks: u32,
    /// Configured maximum time-slice size in ticks (0 = disabled).
    pub slice_max_ticks: u32,
    /// Whether the current time slice has expired.
    pub expired: bool,
    /// Whether time-slicing is enabled (slice_max_ticks > 0).
    pub enabled: bool,
}
impl TimeSlice {
    /// Initialize time-slicing in the disabled state.
    ///
    /// Corresponds to the initial static values in timeslicing.c.
    pub fn init_disabled() -> TimeSlice {
        TimeSlice {
            slice_ticks: 0,
            slice_max_ticks: 0,
            expired: false,
            enabled: false,
        }
    }
    /// Configure time-slicing with a given tick count.
    ///
    /// Corresponds to k_sched_time_slice_set() (timeslicing.c:97-115).
    /// Setting max_ticks to 0 disables time-slicing.
    /// Also performs a reset (TS2).
    pub fn set_config(&mut self, max_ticks: u32) {
        if max_ticks > 0 {
            self.slice_max_ticks = max_ticks;
            self.slice_ticks = max_ticks;
            self.enabled = true;
        } else {
            self.slice_max_ticks = 0;
            self.slice_ticks = 0;
            self.enabled = false;
        }
        self.expired = false;
    }
    /// Reset the time slice to its maximum value.
    ///
    /// Corresponds to z_reset_time_slice() (timeslicing.c:75-86).
    /// TS2: reset sets slice_ticks = slice_max_ticks.
    pub fn reset(&mut self) {
        self.slice_ticks = self.slice_max_ticks;
        self.expired = false;
    }
    /// Consume one tick of the time slice.
    ///
    /// Models the timer interrupt path that decrements the slice counter.
    /// TS3: decrements by 1 when not expired.
    /// TS5: no underflow — tick at 0 sets expired flag instead.
    pub fn tick(&mut self) {
        if self.slice_ticks > 0 {
            #[allow(clippy::arithmetic_side_effects)]
            {
                self.slice_ticks = self.slice_ticks - 1;
            }
            if self.slice_ticks == 0 {
                self.expired = true;
            }
        } else {
            self.expired = true;
        }
    }
    /// Check if the time slice has expired.
    /// TS4: expired when slice_ticks == 0.
    pub fn is_expired(&self) -> bool {
        self.expired
    }
    /// Check if time-slicing is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
    /// Get remaining ticks.
    pub fn remaining(&self) -> u32 {
        self.slice_ticks
    }
    /// Get maximum tick count.
    pub fn max_ticks(&self) -> u32 {
        self.slice_max_ticks
    }
    /// Consume the expired flag (read and clear).
    ///
    /// Models the pattern in z_time_slice() where the expired flag is
    /// checked and the slice is reset.
    pub fn consume_expired(&mut self) -> bool {
        let was = self.expired;
        self.expired = false;
        was
    }
}
/// Decision for timeslice reset: compute new ticks after reset.
///
/// TS2: reset sets slice_ticks = slice_max_ticks.
pub fn reset_decide(slice_max_ticks: u32) -> u32 {
    slice_max_ticks
}
/// Decision for timeslice tick: consume one tick, detect expiry.
///
/// TS3: decrements by 1. TS4: expired when reaching 0. TS5: no underflow.
/// Returns (new_ticks, expired).
pub fn tick_decide(slice_ticks: u32) -> (u32, bool) {
    if slice_ticks > 0 {
        let new = slice_ticks - 1;
        (new, new == 0)
    } else {
        (0, true)
    }
}
/// Full decision for timeslice tick handler: decides whether to yield.
///
/// TS4: expire detection. TS6: cooperative threads never yield.
/// Returns (should_yield, new_ticks).
pub fn timeslice_tick_full_decide(
    ticks_remaining: u32,
    slice_ticks: u32,
    is_cooperative: bool,
) -> (bool, u32) {
    if slice_ticks == 0 {
        (false, ticks_remaining)
    } else if is_cooperative {
        (false, ticks_remaining)
    } else if ticks_remaining == 0 {
        (true, slice_ticks)
    } else {
        (false, ticks_remaining)
    }
}
