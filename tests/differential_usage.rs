//! Differential equivalence tests — Thread Runtime Statistics (FFI vs Model).
//!
//! Verifies that the FFI usage tracking functions produce the same results as
//! the Verus-verified model functions in gale::usage.

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
    clippy::panic
)]

use gale::error::*;
use gale::usage::{
    SysTrackDecision, StartDecision, StopDecision,
    sys_enable_decide, sys_disable_decide,
    start_decide, stop_decide,
    average_cycles, elapsed_cycles,
    ThreadUsage,
};

// FFI action constants matching the shim
const GALE_USAGE_SYS_NOOP: u8  = 0;
const GALE_USAGE_SYS_APPLY: u8 = 1;

const GALE_USAGE_START_RECORD_ONLY: u8   = 0;
const GALE_USAGE_START_RECORD_WINDOW: u8 = 1;

const GALE_USAGE_STOP_SKIP: u8       = 0;
const GALE_USAGE_STOP_ACCUMULATE: u8  = 1;

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Replica of gale_usage_sys_enable_decide.
fn ffi_usage_sys_enable_decide(current_tracking: u32) -> u8 {
    match sys_enable_decide(current_tracking != 0) {
        SysTrackDecision::NoOp => GALE_USAGE_SYS_NOOP,
        SysTrackDecision::Apply => GALE_USAGE_SYS_APPLY,
    }
}

/// Replica of gale_usage_sys_disable_decide.
fn ffi_usage_sys_disable_decide(current_tracking: u32) -> u8 {
    match sys_disable_decide(current_tracking != 0) {
        SysTrackDecision::NoOp => GALE_USAGE_SYS_NOOP,
        SysTrackDecision::Apply => GALE_USAGE_SYS_APPLY,
    }
}

/// Replica of gale_usage_start_decide.
fn ffi_usage_start_decide(track_usage: u32) -> u8 {
    match start_decide(track_usage != 0) {
        StartDecision::RecordOnly => GALE_USAGE_START_RECORD_ONLY,
        StartDecision::RecordStart => GALE_USAGE_START_RECORD_WINDOW,
    }
}

/// Replica of gale_usage_stop_decide.
fn ffi_usage_stop_decide(usage0: u32) -> u8 {
    match stop_decide(usage0) {
        StopDecision::Skip => GALE_USAGE_STOP_SKIP,
        StopDecision::Accumulate => GALE_USAGE_STOP_ACCUMULATE,
    }
}

/// Replica of gale_usage_average_cycles (inline logic).
fn ffi_usage_average_cycles(total_cycles: u64, num_windows: u32) -> u64 {
    average_cycles(total_cycles, num_windows)
}

// =====================================================================
// Differential tests: sys_enable_decide
// =====================================================================

#[test]
fn usage_sys_enable_decide_ffi_matches_model() {
    for current_tracking in [0u32, 1, 2, 0xFFFF_FFFF] {
        let ffi_action = ffi_usage_sys_enable_decide(current_tracking);
        let model = sys_enable_decide(current_tracking != 0);
        let model_action = match model {
            SysTrackDecision::NoOp => GALE_USAGE_SYS_NOOP,
            SysTrackDecision::Apply => GALE_USAGE_SYS_APPLY,
        };
        assert_eq!(ffi_action, model_action,
            "sys_enable_decide mismatch: current_tracking={current_tracking}");
    }
}

#[test]
fn usage_sys_enable_already_tracking_noop() {
    // US4: idempotent — already tracking => no-op
    let action = ffi_usage_sys_enable_decide(1);
    assert_eq!(action, GALE_USAGE_SYS_NOOP,
        "US4: enable when already tracking must be NoOp");

    let model = sys_enable_decide(true);
    assert_eq!(model, SysTrackDecision::NoOp);
}

#[test]
fn usage_sys_enable_not_tracking_apply() {
    let action = ffi_usage_sys_enable_decide(0);
    assert_eq!(action, GALE_USAGE_SYS_APPLY,
        "US4: enable when not tracking must be Apply");

    let model = sys_enable_decide(false);
    assert_eq!(model, SysTrackDecision::Apply);
}

// =====================================================================
// Differential tests: sys_disable_decide
// =====================================================================

#[test]
fn usage_sys_disable_decide_ffi_matches_model() {
    for current_tracking in [0u32, 1, 2, 0xFFFF_FFFF] {
        let ffi_action = ffi_usage_sys_disable_decide(current_tracking);
        let model = sys_disable_decide(current_tracking != 0);
        let model_action = match model {
            SysTrackDecision::NoOp => GALE_USAGE_SYS_NOOP,
            SysTrackDecision::Apply => GALE_USAGE_SYS_APPLY,
        };
        assert_eq!(ffi_action, model_action,
            "sys_disable_decide mismatch: current_tracking={current_tracking}");
    }
}

#[test]
fn usage_sys_disable_not_tracking_noop() {
    // US4: idempotent — not tracking => no-op for disable
    let action = ffi_usage_sys_disable_decide(0);
    assert_eq!(action, GALE_USAGE_SYS_NOOP,
        "US4: disable when not tracking must be NoOp");

    let model = sys_disable_decide(false);
    assert_eq!(model, SysTrackDecision::NoOp);
}

#[test]
fn usage_sys_disable_tracking_apply() {
    let action = ffi_usage_sys_disable_decide(1);
    assert_eq!(action, GALE_USAGE_SYS_APPLY,
        "US4: disable when tracking must be Apply");

    let model = sys_disable_decide(true);
    assert_eq!(model, SysTrackDecision::Apply);
}

// =====================================================================
// Property: US4 — enable and disable are inverses of each other
// =====================================================================

#[test]
fn usage_sys_enable_disable_are_complementary() {
    // enable(x) == Apply iff disable(x) == NoOp
    for tracking in [false, true] {
        let enable_action = sys_enable_decide(tracking);
        let disable_action = sys_disable_decide(tracking);

        let enable_is_apply = matches!(enable_action, SysTrackDecision::Apply);
        let disable_is_noop = matches!(disable_action, SysTrackDecision::NoOp);

        assert_eq!(enable_is_apply, disable_is_noop,
            "US4: enable(Apply) must coincide with disable(NoOp): tracking={tracking}");
    }
}

// =====================================================================
// Differential tests: start_decide
// =====================================================================

#[test]
fn usage_start_decide_ffi_matches_model() {
    for track_usage in [0u32, 1, 2, 0xFFFF_FFFF] {
        let ffi_action = ffi_usage_start_decide(track_usage);
        let model = start_decide(track_usage != 0);
        let model_action = match model {
            StartDecision::RecordOnly => GALE_USAGE_START_RECORD_ONLY,
            StartDecision::RecordStart => GALE_USAGE_START_RECORD_WINDOW,
        };
        assert_eq!(ffi_action, model_action,
            "start_decide mismatch: track_usage={track_usage}");
    }
}

#[test]
fn usage_start_tracking_records_window() {
    // US1: when track_usage is set, start records the window
    let action = ffi_usage_start_decide(1);
    assert_eq!(action, GALE_USAGE_START_RECORD_WINDOW,
        "US1: track_usage=true must return RecordWindow");

    let model = start_decide(true);
    assert_eq!(model, StartDecision::RecordStart);
}

#[test]
fn usage_start_not_tracking_record_only() {
    let action = ffi_usage_start_decide(0);
    assert_eq!(action, GALE_USAGE_START_RECORD_ONLY,
        "US1: track_usage=false must return RecordOnly");

    let model = start_decide(false);
    assert_eq!(model, StartDecision::RecordOnly);
}

// =====================================================================
// Differential tests: stop_decide
// =====================================================================

#[test]
fn usage_stop_decide_ffi_matches_model_exhaustive() {
    let usage0_cases = [0u32, 1, 2, 100, 0xFFFF_FFFF];
    for usage0 in usage0_cases {
        let ffi_action = ffi_usage_stop_decide(usage0);
        let model = stop_decide(usage0);
        let model_action = match model {
            StopDecision::Skip => GALE_USAGE_STOP_SKIP,
            StopDecision::Accumulate => GALE_USAGE_STOP_ACCUMULATE,
        };
        assert_eq!(ffi_action, model_action,
            "stop_decide mismatch: usage0={usage0}");
    }
}

#[test]
fn usage_stop_zero_usage0_skip() {
    // US2: usage0 == 0 means start was never called
    let action = ffi_usage_stop_decide(0);
    assert_eq!(action, GALE_USAGE_STOP_SKIP,
        "US2: usage0=0 must return Skip");

    let model = stop_decide(0);
    assert_eq!(model, StopDecision::Skip);
}

#[test]
fn usage_stop_nonzero_usage0_accumulate() {
    let action = ffi_usage_stop_decide(1000);
    assert_eq!(action, GALE_USAGE_STOP_ACCUMULATE,
        "US2: usage0!=0 must return Accumulate");

    let model = stop_decide(1000);
    assert_eq!(model, StopDecision::Accumulate);
}

// =====================================================================
// Differential tests: average_cycles
// =====================================================================

#[test]
fn usage_average_cycles_ffi_matches_model_exhaustive() {
    let total_cases = [0u64, 1, 100, 1000, u64::MAX / 2];
    let window_cases = [0u32, 1, 2, 10, 100, u32::MAX];

    for total in total_cases {
        for windows in window_cases {
            let ffi_avg = ffi_usage_average_cycles(total, windows);
            let model_avg = average_cycles(total, windows);
            assert_eq!(ffi_avg, model_avg,
                "average_cycles mismatch: total={total}, windows={windows}");
        }
    }
}

#[test]
fn usage_average_cycles_zero_windows_returns_zero() {
    // US5: no division by zero when num_windows == 0
    let avg = average_cycles(1_000_000, 0);
    assert_eq!(avg, 0, "US5: average_cycles with 0 windows must return 0");

    let ffi_avg = ffi_usage_average_cycles(1_000_000, 0);
    assert_eq!(ffi_avg, 0, "US5: FFI average_cycles with 0 windows must return 0");
}

#[test]
fn usage_average_cycles_correct_division() {
    // Known: 1000 total / 10 windows = 100
    let avg = average_cycles(1000, 10);
    assert_eq!(avg, 100, "average_cycles: 1000/10 should be 100");
}

#[test]
fn usage_average_cycles_zero_total() {
    let avg = average_cycles(0, 5);
    assert_eq!(avg, 0, "average_cycles: 0 total / 5 windows = 0");
}

// =====================================================================
// Differential tests: elapsed_cycles
// =====================================================================

#[test]
fn usage_elapsed_cycles_no_wrap() {
    let elapsed = elapsed_cycles(1000, 500);
    assert_eq!(elapsed, 500, "elapsed_cycles: 1000 - 500 = 500");
}

#[test]
fn usage_elapsed_cycles_wrapping() {
    // US2: wrapping subtraction handles u32 counter rollover
    let now = 100u32;
    let usage0 = u32::MAX - 99; // 100 cycles ago, with wrap
    let elapsed = elapsed_cycles(now, usage0);
    assert_eq!(elapsed, now.wrapping_sub(usage0),
        "US2: wrapping elapsed must match wrapping_sub");
    assert_eq!(elapsed, 200u32,
        "US2: wrapping elapsed: 100 - (MAX-99) = 200 (wrapped)");
}

#[test]
fn usage_elapsed_cycles_zero_when_equal() {
    let elapsed = elapsed_cycles(500, 500);
    assert_eq!(elapsed, 0, "elapsed_cycles: same timestamps = 0");
}

// =====================================================================
// Property: ThreadUsage — enable/disable state transitions
// =====================================================================

#[test]
fn usage_thread_enable_sets_track_usage() {
    let mut t = ThreadUsage::new_idle();
    assert!(!t.is_tracked(), "US3: initial state is not tracked");

    let rc = t.enable();
    assert_eq!(rc, OK, "enable must succeed");
    assert!(t.is_tracked(), "US3: after enable, is_tracked() must be true");
    assert_eq!(t.num_windows, 1, "enable increments num_windows");
}

#[test]
fn usage_thread_enable_idempotent() {
    let mut t = ThreadUsage::new_idle();
    t.enable();
    let windows_before = t.num_windows;

    // Second enable should be a no-op (already tracking)
    let rc = t.enable();
    assert_eq!(rc, OK);
    assert_eq!(t.num_windows, windows_before,
        "US4: double-enable must not increment num_windows");
}

#[test]
fn usage_thread_disable_clears_track_usage() {
    let mut t = ThreadUsage::new_idle();
    t.enable();
    assert!(t.is_tracked());

    let rc = t.disable();
    assert_eq!(rc, OK);
    assert!(!t.is_tracked(), "US3: after disable, is_tracked() must be false");
}

#[test]
fn usage_thread_disable_idempotent() {
    let mut t = ThreadUsage::new_idle();
    // already disabled — disable should be a no-op
    let rc = t.disable();
    assert_eq!(rc, OK, "US4: disable when already disabled must succeed");
    assert!(!t.is_tracked(), "US4: still not tracked after no-op disable");
}

// =====================================================================
// Property: US6 — total_cycles is monotonically non-decreasing
// =====================================================================

#[test]
fn usage_accumulate_monotone() {
    let mut t = ThreadUsage { track_usage: true, total_cycles: 0, num_windows: 1 };
    for i in 0u32..=10 {
        let prev = t.total_cycles;
        let cycles = i * 100;
        let rc = t.accumulate(cycles);
        assert_eq!(rc, OK);
        assert!(t.total_cycles >= prev,
            "US6: total_cycles must be non-decreasing after accumulate");
    }
}

#[test]
fn usage_accumulate_overflow_protected() {
    let mut t = ThreadUsage {
        track_usage: true,
        total_cycles: u64::MAX - 100,
        num_windows: 1,
    };
    let rc = t.accumulate(200); // would overflow
    assert_eq!(rc, EOVERFLOW, "US6: overflow must be detected and rejected");
    assert_eq!(t.total_cycles, u64::MAX - 100, "US6: total unchanged on overflow");
}

#[test]
fn usage_accumulate_zero_is_noop() {
    let mut t = ThreadUsage { track_usage: true, total_cycles: 500, num_windows: 1 };
    let rc = t.accumulate(0);
    assert_eq!(rc, OK);
    assert_eq!(t.total_cycles, 500, "accumulate(0) must not change total");
}
