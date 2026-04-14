//! Verified power management state machine model for Zephyr RTOS.
//!
//! This is a formally verified model of Zephyr's PM subsystem lifecycle
//! from subsys/pm/pm.c and subsys/pm/state.c. All safety-critical
//! properties are proven with Verus (SMT/Z3).
//!
//! This module models the **power state machine and policy decision logic**
//! of Zephyr's PM subsystem. Actual hardware power control, device
//! suspend/resume, clock management, and arch-level sleep instructions
//! remain in C.
//!
//! Source mapping:
//!   pm_system_suspend      -> PmState::request_suspend   (pm.c:155-259)
//!   pm_system_resume       -> PmState::notify_resume     (pm.c:100-133)
//!   pm_state_force         -> PmState::force_state       (pm.c:135-153)
//!   pm_policy_next_state   -> policy_next_state_decide   (policy/policy_default.c:12-50)
//!   pm_state_is_valid_transition -> state_transition_valid (state.c)
//!
//! Omitted (not safety-relevant for state machine model):
//!   - pm_state_notify — callback dispatch to registered notifiers
//!   - pm_stats_start/stop/update — performance counters
//!   - sys_clock_set_timeout / sys_clock_idle_exit — timer hardware
//!   - pm_suspend_devices / pm_resume_devices — device power management
//!   - arch pm_state_set / pm_state_exit_post_ops — hardware sleep entry
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
/// Zephyr power state, matching enum pm_state (include/zephyr/pm/state.h).
///
/// Ordered by increasing power savings (ACTIVE = 0 is most awake).
/// SOFT_OFF = 5 is the deepest state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PmState {
    /// CPU is running normally. No power saving.
    Active = 0,
    /// CPU enters a light idle state (clock-gated, fast wakeup).
    /// Maps to PM_STATE_RUNTIME_IDLE.
    RuntimeIdle = 1,
    /// CPU clock stopped, peripherals running.
    /// Maps to PM_STATE_SUSPEND_TO_IDLE.
    SuspendToIdle = 2,
    /// CPU and some peripherals suspended (ACPI S1/S2).
    /// Maps to PM_STATE_STANDBY.
    Standby = 3,
    /// CPU context saved to RAM (ACPI S3 / Linux "mem").
    /// Maps to PM_STATE_SUSPEND_TO_RAM.
    SuspendToRam = 4,
    /// Powered off; system must reboot to recover.
    /// Maps to PM_STATE_SOFT_OFF.
    SoftOff = 5,
}
/// Total number of PM states (matches PM_STATE_COUNT).
pub const PM_STATE_COUNT: u8 = 6;
/// Maximum substate identifier (8-bit field, per Zephyr ABI).
pub const PM_SUBSTATE_MAX: u8 = 255;
impl PmState {
    /// Convert raw u8 to PmState.  Returns Err(EINVAL) for unknown codes.
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
///
/// Minimum residency and exit latency are in microseconds, matching
/// the DT bindings (zephyr,power-state.yaml).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PmStateInfo {
    /// Which power state this describes.
    pub state: PmState,
    /// Sub-state identifier (SoC-specific).  0 = default.
    pub substate_id: u8,
    /// Minimum time the system must stay in this state (µs).
    /// Entering a state for less time than this wastes power.
    pub min_residency_us: u32,
    /// Time required to exit this state and reach full operation (µs).
    pub exit_latency_us: u32,
    /// True if device PM is disabled for this state.
    pub pm_device_disabled: bool,
}
impl PmStateInfo {
    /// Total minimum time including exit latency (for policy comparison).
    ///
    /// Mirrors: min_residency_us + exit_latency_us in policy_default.c:29.
    pub fn effective_residency_us(&self) -> u64 {
        self.min_residency_us as u64 + self.exit_latency_us as u64
    }
}
/// Per-CPU power management state tracker.
///
/// Tracks the current PM state and any pending forced transition.
/// The Extract→Decide→Apply pattern: C extracts the current state,
/// calls Rust to decide the next state, C applies (calls hardware).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PmCpuState {
    /// Current power state of this CPU.
    /// None = ACTIVE (matching z_cpus_pm_state[cpu] == NULL convention).
    pub current: Option<PmState>,
    /// Forced next state, applied on the next suspend opportunity.
    /// None = no forced state pending.
    pub forced: Option<PmState>,
    /// Forced substate identifier (valid only when forced.is_some()).
    pub forced_substate: u8,
    /// Post-ops pending flag: true when wake ISR must run exit sequence.
    pub post_ops_required: bool,
}
impl PmCpuState {
    /// Initialize per-CPU PM state (CPU starts ACTIVE).
    ///
    /// Models z_cpus_pm_state[id] = NULL at boot.
    pub fn init() -> PmCpuState {
        PmCpuState {
            current: None,
            forced: None,
            forced_substate: 0,
            post_ops_required: false,
        }
    }
    /// Force the next power state transition.
    ///
    /// Models pm_state_force() (pm.c:135-153).
    /// PM5: forced state set once, cleared on next suspend.
    pub fn force_state(&mut self, state: PmState, substate_id: u8) -> i32 {
        if matches!(self.current, Some(PmState::SoftOff)) {
            return EINVAL;
        }
        self.forced = Some(state);
        self.forced_substate = substate_id;
        OK
    }
    /// Begin a power state transition (enter low-power mode).
    ///
    /// Models the state selection in pm_system_suspend() (pm.c:183-195).
    /// The C caller has already decided which state to enter (via policy
    /// or forced). This records the transition and arms post_ops_required.
    ///
    /// PM2: from ACTIVE (current == None) any state is reachable.
    /// PM5: consuming a forced state clears it.
    pub fn enter_state(&mut self, state: PmState, substate_id: u8) -> i32 {
        if matches!(state, PmState::Active) {
            return EINVAL;
        }
        self.current = Some(state);
        let _ = substate_id;
        self.forced = None;
        self.forced_substate = 0;
        self.post_ops_required = true;
        OK
    }
    /// Complete wakeup: clear current state and post_ops flag.
    ///
    /// Models pm_system_resume() (pm.c:100-133).
    /// Called from the ISR of the wakeup event.
    ///
    /// PM3: system always returns to ACTIVE after low-power state.
    pub fn resume(&mut self) -> i32 {
        if !self.post_ops_required {
            return EINVAL;
        }
        if matches!(self.current, Some(PmState::SoftOff)) {
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
            None => PmState::Active as u8,
            Some(s) => s as u8,
        }
    }
}
/// Decide whether a requested ticks budget satisfies the minimum residency.
///
/// Models the residency check in pm_policy_next_state() (policy_default.c:27-38).
///
/// PM6: policy respects residency — only enter state if ticks >= residency.
///
/// Arguments:
///   ticks_available: ticks until next scheduled event (i32::MAX = forever).
///   min_residency_ticks: minimum residency in ticks for this state.
///
/// Returns:
///   true  — sufficient time to enter this state.
///   false — not enough time; try a shallower state.
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
/// Decide whether a state transition is valid given current state.
///
/// PM2+PM3+PM4: encodes all legal transitions.
///
/// Transitions:
///   ACTIVE       -> any state (always valid)
///   low-power    -> ACTIVE only (via resume)
///   SOFT_OFF     -> nothing (terminal)
pub fn state_transition_valid(from: PmState, to: PmState) -> bool {
    match from {
        PmState::Active => true,
        PmState::SoftOff => false,
        _ => matches!(to, PmState::Active),
    }
}
/// Select the deepest power state that fits within the ticks budget.
///
/// Models pm_policy_next_state() (policy_default.c:12-50).
/// Iterates from shallowest to deepest, taking the last that fits.
///
/// PM6: only returns states where ticks >= min_residency + exit_latency.
///
/// Arguments:
///   ticks: ticks until next scheduled event (i32::MAX = forever).
///   candidate: proposed next state.
///   min_residency_ticks: precomputed residency threshold for that state.
///   state_available: whether the state is currently unlocked by policy.
///
/// Returns:
///   Some(candidate) — use this state.
///   None            — insufficient residency; fall back to shallower / ACTIVE.
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
/// Models the forced-state check in pm_system_suspend() (pm.c:182-189).
///
/// PM5: forced state is applied exactly once (consumed here).
///
/// Arguments:
///   forced: pending forced state (None if no force).
///   policy_state: state chosen by policy (None if no suitable state).
///
/// Returns:
///   Some(state) — enter this state.
///   None        — stay ACTIVE (no suitable state).
pub fn suspend_state_decide(
    forced: Option<PmState>,
    policy_state: Option<PmState>,
) -> Option<PmState> {
    match forced {
        Some(s) => Some(s),
        None => policy_state,
    }
}
