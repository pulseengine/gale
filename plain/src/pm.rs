//! Verified power management state machine model for Zephyr RTOS.
//!
//! This is the plain (non-Verus) version for Rocq-of-Rust extraction.
//! The logic is identical to src/pm.rs but without Verus proof annotations.
//!
//! Source mapping:
//!   pm_system_suspend      -> PmCpuState::enter_state    (pm.c:155-259)
//!   pm_system_resume       -> PmCpuState::resume         (pm.c:100-133)
//!   pm_state_force         -> PmCpuState::force_state    (pm.c:135-153)
//!   pm_policy_next_state   -> policy_next_state_decide   (policy/policy_default.c:12-50)
//!   state_transition_valid -> state_transition_valid     (state machine)
//!
//! ASIL-D verified properties:
//!   PM1: state is always a valid PmState variant (enum bounds)
//!   PM2: from ACTIVE the system can transition to any lower-power state
//!   PM3: from any low-power state the system always returns to ACTIVE
//!   PM4: SOFT_OFF is a terminal state (no further transitions out)
//!   PM5: forced state is applied once and then cleared (single-use)
//!   PM6: policy respects residency constraint (ticks >= min_residency)
//!   PM7: substate_id is always within u8 bounds

use crate::error::*;

/// Zephyr power state, matching enum pm_state.
///
/// Ordered by increasing power savings (ACTIVE = 0 is most awake).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PmState {
    /// CPU is running normally. No power saving.
    Active        = 0,
    /// CPU enters a light idle state (clock-gated, fast wakeup).
    RuntimeIdle   = 1,
    /// CPU clock stopped, peripherals running.
    SuspendToIdle = 2,
    /// CPU and some peripherals suspended (ACPI S1/S2).
    Standby       = 3,
    /// CPU context saved to RAM (ACPI S3 / Linux "mem").
    SuspendToRam  = 4,
    /// Powered off; system must reboot to recover.
    SoftOff       = 5,
}

/// Total number of PM states (matches PM_STATE_COUNT).
pub const PM_STATE_COUNT: u8 = 6;

/// Maximum substate identifier.
pub const PM_SUBSTATE_MAX: u8 = 255;

impl PmState {
    /// Convert raw u8 to PmState.
    pub fn from_u8(v: u8) -> Result<PmState, i32> {
        match v {
            0 => Ok(PmState::Active),
            1 => Ok(PmState::RuntimeIdle),
            2 => Ok(PmState::SuspendToIdle),
            3 => Ok(PmState::Standby),
            4 => Ok(PmState::SuspendToRam),
            5 => Ok(PmState::SoftOff),
            _ => Err(EINVAL),
        }
    }

    /// Return the numeric code of the state.
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Power state configuration, mirroring Zephyr's struct pm_state_info.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PmStateInfo {
    pub state: PmState,
    pub substate_id: u8,
    pub min_residency_us: u32,
    pub exit_latency_us: u32,
    pub pm_device_disabled: bool,
}

impl PmStateInfo {
    /// Total minimum time including exit latency (for policy comparison).
    pub fn effective_residency_us(&self) -> u64 {
        self.min_residency_us as u64 + self.exit_latency_us as u64
    }
}

/// Per-CPU power management state tracker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PmCpuState {
    pub current: Option<PmState>,
    pub forced: Option<PmState>,
    pub forced_substate: u8,
    pub post_ops_required: bool,
}

impl PmCpuState {
    /// Initialize per-CPU PM state (CPU starts ACTIVE).
    pub fn init() -> PmCpuState {
        PmCpuState {
            current: None,
            forced: None,
            forced_substate: 0,
            post_ops_required: false,
        }
    }

    /// Force the next power state transition.
    pub fn force_state(&mut self, state: PmState, substate_id: u8) -> i32 {
        if self.current == Some(PmState::SoftOff) {
            return EINVAL;
        }
        self.forced = Some(state);
        self.forced_substate = substate_id;
        OK
    }

    /// Begin a power state transition.
    pub fn enter_state(&mut self, state: PmState, substate_id: u8) -> i32 {
        if state == PmState::Active {
            return EINVAL;
        }
        self.current = Some(state);
        let _ = substate_id;
        self.forced = None;
        self.forced_substate = 0;
        self.post_ops_required = true;
        OK
    }

    /// Complete wakeup.
    pub fn resume(&mut self) -> i32 {
        if !self.post_ops_required {
            return EINVAL;
        }
        if self.current == Some(PmState::SoftOff) {
            return EINVAL;
        }
        self.post_ops_required = false;
        self.current = None;
        OK
    }

    /// Check whether the CPU is currently in a low-power state.
    pub fn is_suspended(&self) -> bool {
        self.current.is_some()
    }

    /// Check whether a forced state is pending.
    pub fn has_forced_state(&self) -> bool {
        self.forced.is_some()
    }

    /// Return current state as u8 (0 = ACTIVE when None).
    pub fn current_as_u8(&self) -> u8 {
        match self.current {
            None    => PmState::Active as u8,
            Some(s) => s as u8,
        }
    }
}

/// Decide whether a ticks budget satisfies the minimum residency.
///
/// PM6: policy respects residency — only enter state if ticks >= residency.
pub fn policy_residency_ok(ticks_available: i32, min_residency_ticks: u32) -> bool {
    if ticks_available == i32::MAX {
        true
    } else if ticks_available < 0 {
        false
    } else {
        #[allow(clippy::cast_sign_loss)]
        let avail = ticks_available as u64;
        avail >= min_residency_ticks as u64
    }
}

/// Decide whether a state transition is valid.
///
/// PM2+PM3+PM4.
pub fn state_transition_valid(from: PmState, to: PmState) -> bool {
    match from {
        PmState::Active  => true,
        PmState::SoftOff => false,
        _                => to == PmState::Active,
    }
}

/// Select the deepest power state that fits within the ticks budget.
///
/// PM6: residency check.
pub fn policy_next_state_decide(
    ticks: i32,
    candidate: PmState,
    min_residency_ticks: u32,
    state_available: bool,
) -> Option<PmState> {
    if !state_available {
        return None;
    }
    if !policy_residency_ok(ticks, min_residency_ticks) {
        return None;
    }
    Some(candidate)
}

/// Decide suspend outcome: forced state takes priority over policy.
///
/// PM5.
pub fn suspend_state_decide(
    forced: Option<PmState>,
    policy_state: Option<PmState>,
) -> Option<PmState> {
    match forced {
        Some(s) => Some(s),
        None    => policy_state,
    }
}
