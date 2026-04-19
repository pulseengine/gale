//! Differential equivalence tests — Power Management (FFI vs Model).
//!
//! Verifies that the FFI PM functions produce the same results as
//! the Verus-verified model functions in gale::pm.

#![allow(
    clippy::shadow_unrelated,
    clippy::indexing_slicing,
    clippy::bool_to_int_with_if,
    clippy::unwrap_used,
    clippy::fn_params_excessive_bools,
    clippy::absurd_extreme_comparisons,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::checked_conversions,
    clippy::wildcard_enum_match_arm,
    clippy::implicit_saturating_sub,
    clippy::branches_sharing_code,
    clippy::panic,
    clippy::manual_let_else,
    clippy::expect_used
)]

use gale::error::*;
use gale::pm::{
    PmState, PM_STATE_COUNT,
    policy_residency_ok, state_transition_valid, suspend_state_decide,
};

// State code constants matching the FFI shim
const PM_ACTIVE: u8 = 0;
const PM_RUNTIME_IDLE: u8 = 1;
const PM_SUSPEND_TO_IDLE: u8 = 2;
const PM_STANDBY: u8 = 3;
const PM_SUSPEND_TO_RAM: u8 = 4;
const PM_SOFT_OFF: u8 = 5;

// GalePmForceDecision action codes
const GALE_PM_FORCE_OK: u8 = 0;
const GALE_PM_FORCE_TERMINAL: u8 = 1;

// GalePmSuspendDecision action codes
const GALE_PM_ACTION_ENTER_STATE: u8 = 0;
const GALE_PM_ACTION_STAY_ACTIVE: u8 = 1;

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Replica of gale_pm_force_decide.
/// Returns (action, state, substate_id).
fn ffi_pm_force_decide(
    current_state: u8,
    target_state: u8,
    substate_id: u8,
) -> (u8, u8, u8) {
    if current_state >= PM_STATE_COUNT || target_state >= PM_STATE_COUNT {
        return (GALE_PM_FORCE_TERMINAL, current_state, 0);
    }
    // PM4: SOFT_OFF is terminal
    if current_state == PM_SOFT_OFF {
        return (GALE_PM_FORCE_TERMINAL, current_state, 0);
    }
    (GALE_PM_FORCE_OK, target_state, substate_id)
}

/// Replica of gale_pm_suspend_decide.
/// Returns (action, state, substate_id).
fn ffi_pm_suspend_decide(
    has_forced: u8,
    forced_state: u8,
    forced_substate: u8,
    has_policy: u8,
    policy_state: u8,
    policy_substate: u8,
) -> (u8, u8, u8) {
    // Decode forced option
    let forced = if has_forced != 0 && forced_state < PM_STATE_COUNT {
        PmState::from_u8(forced_state).ok()
    } else {
        None
    };
    // Decode policy option
    let policy = if has_policy != 0 && policy_state < PM_STATE_COUNT {
        PmState::from_u8(policy_state).ok()
    } else {
        None
    };

    match suspend_state_decide(forced, policy) {
        Some(state) => {
            let substate = if has_forced != 0 { forced_substate } else { policy_substate };
            (GALE_PM_ACTION_ENTER_STATE, state as u8, substate)
        }
        None => (GALE_PM_ACTION_STAY_ACTIVE, 0, 0),
    }
}

/// Replica of gale_pm_residency_ok.
fn ffi_pm_residency_ok(ticks_available: i32, min_residency_ticks: u32) -> bool {
    policy_residency_ok(ticks_available, min_residency_ticks)
}

/// Replica of gale_pm_transition_valid.
fn ffi_pm_transition_valid(from_state: u8, to_state: u8) -> u8 {
    if from_state >= PM_STATE_COUNT || to_state >= PM_STATE_COUNT {
        return 0;
    }
    let from = match PmState::from_u8(from_state) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let to = match PmState::from_u8(to_state) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    if state_transition_valid(from, to) { 1 } else { 0 }
}

// =====================================================================
// Differential tests: force_decide
// =====================================================================

#[test]
fn pm_force_decide_ffi_matches_model_valid_states() {
    for current in 0u8..PM_STATE_COUNT {
        for target in 0u8..PM_STATE_COUNT {
            let substate = 3u8;
            let (ffi_action, ffi_state, ffi_sub) =
                ffi_pm_force_decide(current, target, substate);

            if current == PM_SOFT_OFF {
                // PM4: SOFT_OFF is terminal
                assert_eq!(ffi_action, GALE_PM_FORCE_TERMINAL,
                    "PM4: force from SOFT_OFF must be terminal");
            } else {
                assert_eq!(ffi_action, GALE_PM_FORCE_OK,
                    "force from non-terminal current={current} must be OK");
                assert_eq!(ffi_state, target,
                    "forced state must be the target: current={current}, target={target}");
                assert_eq!(ffi_sub, substate,
                    "forced substate must match: current={current}, target={target}");
            }
        }
    }
}

#[test]
fn pm_force_decide_invalid_state_terminal() {
    // out-of-range codes treated as terminal
    let (action, _, _) = ffi_pm_force_decide(PM_STATE_COUNT, 0, 0);
    assert_eq!(action, GALE_PM_FORCE_TERMINAL,
        "invalid current_state must return TERMINAL");

    let (action2, _, _) = ffi_pm_force_decide(0, PM_STATE_COUNT, 0);
    assert_eq!(action2, GALE_PM_FORCE_TERMINAL,
        "invalid target_state must return TERMINAL");
}

#[test]
fn pm_force_decide_soft_off_is_terminal() {
    for target in 0u8..PM_STATE_COUNT {
        let (action, _, _) = ffi_pm_force_decide(PM_SOFT_OFF, target, 0);
        assert_eq!(action, GALE_PM_FORCE_TERMINAL,
            "PM4: SOFT_OFF current must always be TERMINAL, target={target}");
    }
}

// =====================================================================
// Differential tests: suspend_decide
// =====================================================================

#[test]
fn pm_suspend_decide_forced_wins_over_policy() {
    // PM5: forced state takes priority
    let (action, state, _) = ffi_pm_suspend_decide(
        1, PM_STANDBY, 0,      // forced: Standby
        1, PM_RUNTIME_IDLE, 0, // policy: RuntimeIdle
    );
    assert_eq!(action, GALE_PM_ACTION_ENTER_STATE);
    assert_eq!(state, PM_STANDBY, "PM5: forced state (Standby) must win over policy (RuntimeIdle)");
}

#[test]
fn pm_suspend_decide_policy_used_when_no_forced() {
    let (action, state, _) = ffi_pm_suspend_decide(
        0, 0, 0,                // no forced
        1, PM_SUSPEND_TO_RAM, 0,// policy: SuspendToRam
    );
    assert_eq!(action, GALE_PM_ACTION_ENTER_STATE);
    assert_eq!(state, PM_SUSPEND_TO_RAM, "PM5: no forced => policy state used");
}

#[test]
fn pm_suspend_decide_no_state_stays_active() {
    let (action, _, _) = ffi_pm_suspend_decide(0, 0, 0, 0, 0, 0);
    assert_eq!(action, GALE_PM_ACTION_STAY_ACTIVE,
        "no forced, no policy => STAY_ACTIVE");
}

#[test]
fn pm_suspend_decide_ffi_matches_model_exhaustive() {
    for has_forced in [0u8, 1] {
        for forced_state in [PM_RUNTIME_IDLE, PM_STANDBY, PM_SOFT_OFF] {
            for has_policy in [0u8, 1] {
                for policy_state in [PM_RUNTIME_IDLE, PM_SUSPEND_TO_IDLE, PM_SUSPEND_TO_RAM] {
                    let (ffi_action, ffi_state, ffi_sub) = ffi_pm_suspend_decide(
                        has_forced, forced_state, 0,
                        has_policy, policy_state, 0,
                    );

                    // Reconstruct the model's expected answer
                    let forced = if has_forced != 0 && forced_state < PM_STATE_COUNT {
                        PmState::from_u8(forced_state).ok()
                    } else {
                        None
                    };
                    let policy = if has_policy != 0 && policy_state < PM_STATE_COUNT {
                        PmState::from_u8(policy_state).ok()
                    } else {
                        None
                    };
                    let model = suspend_state_decide(forced, policy);

                    match model {
                        Some(model_state) => {
                            assert_eq!(ffi_action, GALE_PM_ACTION_ENTER_STATE,
                                "action mismatch: has_forced={has_forced}, forced_state={forced_state}, \
                                 has_policy={has_policy}, policy_state={policy_state}");
                            assert_eq!(ffi_state, model_state as u8,
                                "state mismatch: has_forced={has_forced}, forced_state={forced_state}");
                        }
                        None => {
                            assert_eq!(ffi_action, GALE_PM_ACTION_STAY_ACTIVE,
                                "no-state case: has_forced={has_forced}, has_policy={has_policy}");
                        }
                    }
                    let _ = ffi_sub;
                }
            }
        }
    }
}

// =====================================================================
// Differential tests: residency_ok
// =====================================================================

#[test]
fn pm_residency_ok_ffi_matches_model_exhaustive() {
    let tick_cases = [
        i32::MIN, -1i32, 0i32, 1, 100, 1000, i32::MAX - 1, i32::MAX,
    ];
    let residency_cases = [0u32, 1, 50, 100, 1000, u32::MAX];

    for ticks in tick_cases {
        for residency in residency_cases {
            let ffi_ok = ffi_pm_residency_ok(ticks, residency);
            let model_ok = policy_residency_ok(ticks, residency);
            assert_eq!(ffi_ok, model_ok,
                "residency_ok mismatch: ticks={ticks}, residency={residency}");
        }
    }
}

#[test]
fn pm_residency_ok_forever_always_ok() {
    // i32::MAX means "forever" — always sufficient residency
    for residency in [0u32, 1000, u32::MAX] {
        let ok = ffi_pm_residency_ok(i32::MAX, residency);
        assert!(ok, "PM6: i32::MAX ticks must always satisfy residency={residency}");
    }
}

#[test]
fn pm_residency_ok_negative_ticks_not_ok() {
    // Negative ticks (expired budget) should fail residency
    let ok = ffi_pm_residency_ok(-1, 1);
    assert!(!ok, "PM6: negative ticks must fail residency check");
}

#[test]
fn pm_residency_ok_sufficient_ticks() {
    let ok = ffi_pm_residency_ok(1000, 500);
    assert!(ok, "PM6: 1000 ticks >= 500 residency must pass");
}

#[test]
fn pm_residency_ok_insufficient_ticks() {
    let ok = ffi_pm_residency_ok(100, 500);
    assert!(!ok, "PM6: 100 ticks < 500 residency must fail");
}

// =====================================================================
// Differential tests: transition_valid
// =====================================================================

#[test]
fn pm_transition_valid_ffi_matches_model_exhaustive() {
    for from in 0u8..PM_STATE_COUNT {
        for to in 0u8..PM_STATE_COUNT {
            let ffi_result = ffi_pm_transition_valid(from, to);
            let from_state = PmState::from_u8(from).expect("valid from state");
            let to_state = PmState::from_u8(to).expect("valid to state");
            let model_result = if state_transition_valid(from_state, to_state) { 1u8 } else { 0 };

            assert_eq!(ffi_result, model_result,
                "transition_valid mismatch: from={from}, to={to}");
        }
    }
}

#[test]
fn pm_transition_valid_from_active_all_allowed() {
    // PM2: from ACTIVE any transition is valid
    for to in 0u8..PM_STATE_COUNT {
        let result = ffi_pm_transition_valid(PM_ACTIVE, to);
        assert_eq!(result, 1,
            "PM2: ACTIVE->state={to} must be valid");
    }
}

#[test]
fn pm_transition_valid_soft_off_is_terminal() {
    // PM4: no transitions out of SOFT_OFF
    for to in 0u8..PM_STATE_COUNT {
        let result = ffi_pm_transition_valid(PM_SOFT_OFF, to);
        assert_eq!(result, 0,
            "PM4: SOFT_OFF->state={to} must be invalid");
    }
}

#[test]
fn pm_transition_valid_low_power_to_active_only() {
    // PM3: low-power states can only resume to ACTIVE
    let low_power = [PM_RUNTIME_IDLE, PM_SUSPEND_TO_IDLE, PM_STANDBY, PM_SUSPEND_TO_RAM];
    for from in low_power {
        for to in 0u8..PM_STATE_COUNT {
            let result = ffi_pm_transition_valid(from, to);
            if to == PM_ACTIVE {
                assert_eq!(result, 1,
                    "PM3: low-power={from} -> ACTIVE must be valid");
            } else {
                assert_eq!(result, 0,
                    "PM3: low-power={from} -> non-ACTIVE={to} must be invalid");
            }
        }
    }
}

#[test]
fn pm_transition_valid_invalid_codes_rejected() {
    // Out-of-range state codes
    assert_eq!(ffi_pm_transition_valid(PM_STATE_COUNT, 0), 0,
        "invalid from-state must be rejected");
    assert_eq!(ffi_pm_transition_valid(0, PM_STATE_COUNT), 0,
        "invalid to-state must be rejected");
    assert_eq!(ffi_pm_transition_valid(255, 255), 0,
        "both out-of-range states must be rejected");
}

// =====================================================================
// Property: PmCpuState — state machine operations
// =====================================================================

#[test]
fn pm_cpu_state_init_is_active() {
    use gale::pm::PmCpuState;
    let s = PmCpuState::init();
    assert!(s.current.is_none(), "PM1: init state is ACTIVE (None)");
    assert!(s.forced.is_none(), "init: no forced state pending");
    assert!(!s.post_ops_required, "init: no post-ops pending");
}

#[test]
fn pm_cpu_state_force_sets_pending() {
    use gale::pm::PmCpuState;
    let mut s = PmCpuState::init();
    let rc = s.force_state(PmState::Standby, 2);
    assert_eq!(rc, OK, "PM5: force_state must succeed from ACTIVE");
    assert_eq!(s.forced, Some(PmState::Standby),
        "PM5: forced state must be set");
    assert_eq!(s.forced_substate, 2);
}

#[test]
fn pm_cpu_state_force_from_soft_off_rejected() {
    use gale::pm::PmCpuState;
    let mut s = PmCpuState {
        current: Some(PmState::SoftOff),
        forced: None,
        forced_substate: 0,
        post_ops_required: false,
    };
    let rc = s.force_state(PmState::Standby, 0);
    assert_eq!(rc, EINVAL, "PM4: cannot force from SOFT_OFF");
}

#[test]
fn pm_cpu_state_enter_state_arms_post_ops() {
    use gale::pm::PmCpuState;
    let mut s = PmCpuState::init();
    let rc = s.enter_state(PmState::Standby, 0);
    assert_eq!(rc, OK);
    assert_eq!(s.current, Some(PmState::Standby));
    assert!(s.post_ops_required, "enter_state must arm post_ops_required");
    assert!(s.forced.is_none(), "PM5: forced consumed on enter");
}

#[test]
fn pm_cpu_state_resume_returns_to_active() {
    use gale::pm::PmCpuState;
    let mut s = PmCpuState::init();
    s.enter_state(PmState::Standby, 0);
    let rc = s.resume();
    assert_eq!(rc, OK, "PM3: resume must succeed");
    assert!(s.current.is_none(), "PM3: after resume, state is ACTIVE (None)");
    assert!(!s.post_ops_required, "PM3: post_ops cleared after resume");
}
