//! Integration tests for the power management state machine model.
//!
//! Tests cover the ASIL-D verified properties:
//!   PM1: state enum bounds
//!   PM2: ACTIVE can transition to any state
//!   PM3: any non-terminal state can resume to ACTIVE
//!   PM4: SOFT_OFF is terminal
//!   PM5: forced state single-use
//!   PM6: residency policy
//!   PM7: substate within u8 bounds

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::error::*;
use gale::pm::*;

// ======================================================================
// PM1: state enum bounds
// ======================================================================

#[test]
fn pm_state_count_is_six() {
    assert_eq!(PM_STATE_COUNT, 6);
}

#[test]
fn all_states_fit_in_u8() {
    let states = [
        PmState::Active,
        PmState::RuntimeIdle,
        PmState::SuspendToIdle,
        PmState::Standby,
        PmState::SuspendToRam,
        PmState::SoftOff,
    ];
    for s in states {
        assert!(s.as_u8() < PM_STATE_COUNT);
    }
}

#[test]
fn from_u8_roundtrip() {
    for v in 0..PM_STATE_COUNT {
        let s = PmState::from_u8(v).unwrap();
        assert_eq!(s.as_u8(), v);
    }
}

#[test]
fn from_u8_rejects_invalid() {
    assert_eq!(PmState::from_u8(6), Err(EINVAL));
    assert_eq!(PmState::from_u8(255), Err(EINVAL));
}

// ======================================================================
// PM2: ACTIVE transitions to any state
// ======================================================================

#[test]
fn active_can_enter_runtime_idle() {
    let mut s = PmCpuState::init();
    assert_eq!(s.enter_state(PmState::RuntimeIdle, 0), OK);
    assert_eq!(s.current, Some(PmState::RuntimeIdle));
    assert!(s.post_ops_required);
}

#[test]
fn active_can_enter_standby() {
    let mut s = PmCpuState::init();
    assert_eq!(s.enter_state(PmState::Standby, 0), OK);
    assert_eq!(s.current, Some(PmState::Standby));
}

#[test]
fn active_can_enter_suspend_to_idle() {
    let mut s = PmCpuState::init();
    assert_eq!(s.enter_state(PmState::SuspendToIdle, 0), OK);
    assert_eq!(s.current, Some(PmState::SuspendToIdle));
}

#[test]
fn active_can_enter_suspend_to_ram() {
    let mut s = PmCpuState::init();
    assert_eq!(s.enter_state(PmState::SuspendToRam, 2), OK);
    assert_eq!(s.current, Some(PmState::SuspendToRam));
}

#[test]
fn active_can_enter_soft_off() {
    let mut s = PmCpuState::init();
    assert_eq!(s.enter_state(PmState::SoftOff, 0), OK);
    assert_eq!(s.current, Some(PmState::SoftOff));
}

#[test]
fn enter_active_via_enter_state_is_rejected() {
    let mut s = PmCpuState::init();
    // PM2: ACTIVE is the base; entering ACTIVE via enter_state is not a
    // power management operation — use resume() instead.
    assert_eq!(s.enter_state(PmState::Active, 0), EINVAL);
    assert!(s.current.is_none()); // unchanged
}

// ======================================================================
// PM3: resume always returns to ACTIVE
// ======================================================================

#[test]
fn resume_from_runtime_idle() {
    let mut s = PmCpuState::init();
    assert_eq!(s.enter_state(PmState::RuntimeIdle, 0), OK);
    assert_eq!(s.resume(), OK);
    assert!(s.current.is_none());
    assert!(!s.post_ops_required);
}

#[test]
fn resume_from_standby() {
    let mut s = PmCpuState::init();
    assert_eq!(s.enter_state(PmState::Standby, 0), OK);
    assert_eq!(s.resume(), OK);
    assert!(s.current.is_none());
}

#[test]
fn resume_from_suspend_to_ram() {
    let mut s = PmCpuState::init();
    assert_eq!(s.enter_state(PmState::SuspendToRam, 0), OK);
    assert_eq!(s.resume(), OK);
    assert!(s.current.is_none());
}

#[test]
fn resume_when_not_suspended_is_einval() {
    let mut s = PmCpuState::init();
    // CPU was never suspended
    assert_eq!(s.resume(), EINVAL);
    assert!(s.current.is_none()); // still ACTIVE
}

#[test]
fn full_suspend_resume_cycle() {
    let mut s = PmCpuState::init();
    let original = s;
    assert_eq!(s.enter_state(PmState::Standby, 0), OK);
    assert!(s.is_suspended());
    assert_eq!(s.resume(), OK);
    assert!(!s.is_suspended());
    assert_eq!(s, original);
}

// ======================================================================
// PM4: SOFT_OFF is terminal
// ======================================================================

#[test]
fn soft_off_resume_is_rejected() {
    let mut s = PmCpuState::init();
    assert_eq!(s.enter_state(PmState::SoftOff, 0), OK);
    // PM4: no resume from SOFT_OFF
    assert_eq!(s.resume(), EINVAL);
    assert_eq!(s.current, Some(PmState::SoftOff));
}

#[test]
fn soft_off_force_state_is_rejected() {
    let mut s = PmCpuState::init();
    assert_eq!(s.enter_state(PmState::SoftOff, 0), OK);
    // PM4: cannot force a transition from SOFT_OFF
    assert_eq!(s.force_state(PmState::RuntimeIdle, 0), EINVAL);
    assert!(s.forced.is_none());
}

#[test]
fn state_transition_valid_soft_off_is_false() {
    // PM4: SOFT_OFF -> any is invalid
    assert!(!state_transition_valid(PmState::SoftOff, PmState::Active));
    assert!(!state_transition_valid(PmState::SoftOff, PmState::RuntimeIdle));
    assert!(!state_transition_valid(PmState::SoftOff, PmState::SoftOff));
}

// ======================================================================
// PM5: forced state single-use
// ======================================================================

#[test]
fn forced_state_is_consumed_on_enter() {
    let mut s = PmCpuState::init();
    assert_eq!(s.force_state(PmState::Standby, 0), OK);
    assert!(s.has_forced_state());
    // C would call suspend_state_decide, then enter_state
    assert_eq!(s.enter_state(PmState::Standby, 0), OK);
    // PM5: forced cleared after use
    assert!(!s.has_forced_state());
    assert!(s.forced.is_none());
}

#[test]
fn forced_state_takes_priority_over_policy() {
    // PM5: forced state wins
    let decision = suspend_state_decide(Some(PmState::SoftOff), Some(PmState::RuntimeIdle));
    assert_eq!(decision, Some(PmState::SoftOff));
}

#[test]
fn no_forced_state_uses_policy() {
    let decision = suspend_state_decide(None, Some(PmState::Standby));
    assert_eq!(decision, Some(PmState::Standby));
}

#[test]
fn no_forced_no_policy_returns_none() {
    let decision = suspend_state_decide(None, None);
    assert!(decision.is_none());
}

#[test]
fn force_state_sets_substate() {
    let mut s = PmCpuState::init();
    assert_eq!(s.force_state(PmState::SuspendToRam, 42), OK);
    assert_eq!(s.forced, Some(PmState::SuspendToRam));
    assert_eq!(s.forced_substate, 42);
}

#[test]
fn force_state_can_be_overridden() {
    let mut s = PmCpuState::init();
    assert_eq!(s.force_state(PmState::Standby, 0), OK);
    assert_eq!(s.force_state(PmState::SuspendToRam, 1), OK);
    assert_eq!(s.forced, Some(PmState::SuspendToRam));
}

// ======================================================================
// PM6: policy residency
// ======================================================================

#[test]
fn residency_ok_when_forever() {
    assert!(policy_residency_ok(i32::MAX, u32::MAX));
    assert!(policy_residency_ok(i32::MAX, 0));
}

#[test]
fn residency_ok_exact_match() {
    assert!(policy_residency_ok(100, 100));
}

#[test]
fn residency_ok_more_than_enough() {
    assert!(policy_residency_ok(1000, 100));
}

#[test]
fn residency_fails_when_not_enough() {
    assert!(!policy_residency_ok(50, 100));
}

#[test]
fn residency_fails_when_negative() {
    assert!(!policy_residency_ok(-1, 0));
    assert!(!policy_residency_ok(-100, 100));
}

#[test]
fn policy_next_state_decide_ok() {
    let result = policy_next_state_decide(200, PmState::Standby, 100, true);
    assert_eq!(result, Some(PmState::Standby));
}

#[test]
fn policy_next_state_decide_insufficient_residency() {
    let result = policy_next_state_decide(50, PmState::Standby, 100, true);
    assert!(result.is_none());
}

#[test]
fn policy_next_state_decide_state_unavailable() {
    let result = policy_next_state_decide(200, PmState::Standby, 100, false);
    assert!(result.is_none());
}

#[test]
fn policy_next_state_decide_forever_ticks() {
    // i32::MAX means K_TICKS_FOREVER — always satisfies residency
    let result = policy_next_state_decide(i32::MAX, PmState::SuspendToRam, u32::MAX, true);
    assert_eq!(result, Some(PmState::SuspendToRam));
}

// ======================================================================
// PM7: substate within u8 bounds
// ======================================================================

#[test]
fn substate_max_is_u8_max() {
    assert_eq!(PM_SUBSTATE_MAX, 255u8);
}

#[test]
fn force_state_accepts_max_substate() {
    let mut s = PmCpuState::init();
    assert_eq!(s.force_state(PmState::Standby, PM_SUBSTATE_MAX), OK);
    assert_eq!(s.forced_substate, PM_SUBSTATE_MAX);
}

// ======================================================================
// State transition matrix
// ======================================================================

#[test]
fn transition_valid_from_active() {
    // PM2: ACTIVE -> everything is valid
    for v in 0..PM_STATE_COUNT {
        let to = PmState::from_u8(v).unwrap();
        assert!(state_transition_valid(PmState::Active, to), "ACTIVE -> {v}");
    }
}

#[test]
fn transition_valid_from_low_power_only_to_active() {
    let low_power = [
        PmState::RuntimeIdle,
        PmState::SuspendToIdle,
        PmState::Standby,
        PmState::SuspendToRam,
    ];
    for from in low_power {
        // PM3: can resume to ACTIVE
        assert!(state_transition_valid(from, PmState::Active), "{from:?} -> Active");
        // But not to other states (those require returning to ACTIVE first)
        for v in 1..PM_STATE_COUNT {
            let to = PmState::from_u8(v).unwrap();
            assert!(
                !state_transition_valid(from, to),
                "{from:?} -> {to:?} should be invalid"
            );
        }
    }
}

// ======================================================================
// PmStateInfo helpers
// ======================================================================

#[test]
fn effective_residency_sums_correctly() {
    let info = PmStateInfo {
        state: PmState::Standby,
        substate_id: 0,
        min_residency_us: 1000,
        exit_latency_us: 200,
        pm_device_disabled: false,
    };
    assert_eq!(info.effective_residency_us(), 1200u64);
}

#[test]
fn effective_residency_no_overflow_large_values() {
    let info = PmStateInfo {
        state: PmState::SuspendToRam,
        substate_id: 0,
        min_residency_us: u32::MAX,
        exit_latency_us: u32::MAX,
        pm_device_disabled: false,
    };
    // Should not overflow (computed as u64)
    assert_eq!(info.effective_residency_us(), u32::MAX as u64 * 2);
}

// ======================================================================
// current_as_u8 helper
// ======================================================================

#[test]
fn current_as_u8_when_active() {
    let s = PmCpuState::init();
    assert_eq!(s.current_as_u8(), PmState::Active as u8);
    assert_eq!(s.current_as_u8(), 0);
}

#[test]
fn current_as_u8_when_suspended() {
    let mut s = PmCpuState::init();
    assert_eq!(s.enter_state(PmState::SuspendToRam, 0), OK);
    assert_eq!(s.current_as_u8(), PmState::SuspendToRam as u8);
    assert_eq!(s.current_as_u8(), 4);
}

// ======================================================================
// Stress tests
// ======================================================================

#[test]
fn stress_suspend_resume_cycles() {
    let mut s = PmCpuState::init();
    let states = [
        PmState::RuntimeIdle,
        PmState::SuspendToIdle,
        PmState::Standby,
        PmState::SuspendToRam,
    ];
    for _ in 0..20 {
        for &state in &states {
            assert_eq!(s.enter_state(state, 0), OK);
            assert!(s.is_suspended());
            assert_eq!(s.resume(), OK);
            assert!(!s.is_suspended());
        }
    }
}

#[test]
fn stress_forced_state_cycles() {
    let mut s = PmCpuState::init();
    for i in 0u8..50 {
        let state = PmState::from_u8(i % 5 + 1).unwrap(); // 1..5 (skip ACTIVE, SoftOff)
        let target = if state == PmState::SoftOff {
            PmState::Standby
        } else {
            state
        };
            assert_eq!(s.force_state(target, i), OK);
        assert!(s.has_forced_state());
        assert_eq!(s.enter_state(target, i), OK);
        assert!(!s.has_forced_state());
        assert_eq!(s.resume(), OK);
    }
}
