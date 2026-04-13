//! Integration tests for the thread runtime statistics model.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::error::*;
use gale::usage::*;

// ======================================================================
// ThreadUsage — new_idle
// ======================================================================

#[test]
fn new_idle_initial_state() {
    let u = ThreadUsage::new_idle();
    assert!(!u.track_usage);
    assert_eq!(u.total_cycles, 0);
    assert_eq!(u.num_windows, 0);
    assert!(!u.is_tracked());
}

// ======================================================================
// ThreadUsage — enable / disable (US3)
// ======================================================================

#[test]
fn enable_sets_track_usage() {
    let mut u = ThreadUsage::new_idle();
    assert_eq!(u.enable(), OK);
    assert!(u.track_usage);
    assert_eq!(u.num_windows, 1);
    assert_eq!(u.total_cycles, 0);
}

#[test]
fn enable_is_idempotent() {
    let mut u = ThreadUsage::new_idle();
    u.enable();
    let snapshot = u;
    assert_eq!(u.enable(), OK);
    // Second enable: track_usage stays true, num_windows unchanged
    assert_eq!(u, snapshot);
}

#[test]
fn disable_clears_track_usage() {
    let mut u = ThreadUsage::new_idle();
    u.enable();
    assert_eq!(u.disable(), OK);
    assert!(!u.track_usage);
}

#[test]
fn disable_on_already_disabled_is_ok() {
    let mut u = ThreadUsage::new_idle();
    assert_eq!(u.disable(), OK);
    assert!(!u.track_usage);
}

#[test]
fn enable_disable_roundtrip() {
    let mut u = ThreadUsage::new_idle();
    u.enable();
    u.disable();
    assert!(!u.track_usage);
}

#[test]
fn enable_preserves_total_cycles() {
    let mut u = ThreadUsage { track_usage: false, total_cycles: 12345, num_windows: 3 };
    u.enable();
    assert_eq!(u.total_cycles, 12345);
}

// ======================================================================
// ThreadUsage — accumulate (US6)
// ======================================================================

#[test]
fn accumulate_adds_cycles() {
    let mut u = ThreadUsage::new_idle();
    assert_eq!(u.accumulate(1000), OK);
    assert_eq!(u.total_cycles, 1000);
}

#[test]
fn accumulate_is_monotone() {
    let mut u = ThreadUsage::new_idle();
    u.accumulate(500);
    u.accumulate(300);
    u.accumulate(200);
    assert_eq!(u.total_cycles, 1000);
}

#[test]
fn accumulate_overflow_returns_eoverflow() {
    let mut u = ThreadUsage { track_usage: true, total_cycles: u64::MAX, num_windows: 1 };
    assert_eq!(u.accumulate(1), EOVERFLOW);
    // total_cycles unchanged on overflow
    assert_eq!(u.total_cycles, u64::MAX);
}

#[test]
fn accumulate_zero_cycles_is_ok() {
    let mut u = ThreadUsage::new_idle();
    assert_eq!(u.accumulate(0), OK);
    assert_eq!(u.total_cycles, 0);
}

#[test]
fn accumulate_large_value() {
    let mut u = ThreadUsage::new_idle();
    assert_eq!(u.accumulate(u32::MAX), OK);
    assert_eq!(u.total_cycles, u32::MAX as u64);
}

// ======================================================================
// sys_enable_decide / sys_disable_decide (US4)
// ======================================================================

#[test]
fn sys_enable_when_not_tracking_returns_apply() {
    assert_eq!(sys_enable_decide(false), SysTrackDecision::Apply);
}

#[test]
fn sys_enable_when_already_tracking_returns_noop() {
    assert_eq!(sys_enable_decide(true), SysTrackDecision::NoOp);
}

#[test]
fn sys_disable_when_tracking_returns_apply() {
    assert_eq!(sys_disable_decide(true), SysTrackDecision::Apply);
}

#[test]
fn sys_disable_when_not_tracking_returns_noop() {
    assert_eq!(sys_disable_decide(false), SysTrackDecision::NoOp);
}

#[test]
fn sys_enable_idempotent_sequence() {
    // Simulates calling enable twice without actually updating state
    assert_eq!(sys_enable_decide(true), SysTrackDecision::NoOp);
    assert_eq!(sys_enable_decide(true), SysTrackDecision::NoOp);
}

#[test]
fn sys_disable_idempotent_sequence() {
    assert_eq!(sys_disable_decide(false), SysTrackDecision::NoOp);
    assert_eq!(sys_disable_decide(false), SysTrackDecision::NoOp);
}

// ======================================================================
// start_decide (US1)
// ======================================================================

#[test]
fn start_decide_tracking_returns_record_start() {
    assert_eq!(start_decide(true), StartDecision::RecordStart);
}

#[test]
fn start_decide_not_tracking_returns_record_only() {
    assert_eq!(start_decide(false), StartDecision::RecordOnly);
}

// ======================================================================
// stop_decide (US2)
// ======================================================================

#[test]
fn stop_decide_zero_usage0_returns_skip() {
    assert_eq!(stop_decide(0), StopDecision::Skip);
}

#[test]
fn stop_decide_nonzero_usage0_returns_accumulate() {
    assert_eq!(stop_decide(1), StopDecision::Accumulate);
    assert_eq!(stop_decide(u32::MAX), StopDecision::Accumulate);
    assert_eq!(stop_decide(0x12345678), StopDecision::Accumulate);
}

// ======================================================================
// average_cycles (US5)
// ======================================================================

#[test]
fn average_cycles_zero_windows_returns_zero() {
    // US5: no division by zero
    assert_eq!(average_cycles(0, 0), 0);
    assert_eq!(average_cycles(u64::MAX, 0), 0);
    assert_eq!(average_cycles(1_000_000, 0), 0);
}

#[test]
fn average_cycles_one_window() {
    assert_eq!(average_cycles(1000, 1), 1000);
}

#[test]
fn average_cycles_exact_division() {
    assert_eq!(average_cycles(1000, 10), 100);
}

#[test]
fn average_cycles_truncating_division() {
    // 1001 / 10 = 100 (truncating)
    assert_eq!(average_cycles(1001, 10), 100);
}

#[test]
fn average_cycles_large_windows() {
    assert_eq!(average_cycles(u32::MAX as u64, u32::MAX), 1);
}

// ======================================================================
// elapsed_cycles (US2)
// ======================================================================

#[test]
fn elapsed_cycles_simple() {
    assert_eq!(elapsed_cycles(200, 100), 100);
}

#[test]
fn elapsed_cycles_wrap_around() {
    // Simulates u32 counter wrap: now wrapped past 0
    let now: u32 = 10;
    let usage0: u32 = u32::MAX - 5; // 4294967290
    // Elapsed = 10 - 4294967290 wrapping = 16
    assert_eq!(elapsed_cycles(now, usage0), 16);
}

#[test]
fn elapsed_cycles_same_value() {
    assert_eq!(elapsed_cycles(42, 42), 0);
}

#[test]
fn elapsed_cycles_zero_usage0() {
    assert_eq!(elapsed_cycles(1000, 0), 1000);
}

// ======================================================================
// Scenario: typical thread lifecycle
// ======================================================================

#[test]
fn scenario_enable_run_accumulate_disable() {
    let mut u = ThreadUsage::new_idle();

    // Enable tracking before thread runs
    assert_eq!(u.enable(), OK);
    assert!(u.is_tracked());
    assert_eq!(u.num_windows, 1);

    // Thread runs: accumulate some cycles
    let usage0: u32 = 1000;
    let now: u32 = 5000;
    let cycles = elapsed_cycles(now, usage0);
    assert_eq!(cycles, 4000);
    assert_eq!(u.accumulate(cycles), OK);
    assert_eq!(u.total_cycles, 4000);

    // Thread runs again
    let usage0b: u32 = 6000;
    let nowb: u32 = 8000;
    let cycles2 = elapsed_cycles(nowb, usage0b);
    assert_eq!(u.accumulate(cycles2), OK);
    assert_eq!(u.total_cycles, 6000);

    // Average over 1 window
    let avg = average_cycles(u.total_cycles, u.num_windows);
    assert_eq!(avg, 6000);

    // Disable tracking
    assert_eq!(u.disable(), OK);
    assert!(!u.is_tracked());
    // Total cycles preserved
    assert_eq!(u.total_cycles, 6000);
}

#[test]
fn scenario_start_stop_decisions() {
    // Simulate z_sched_usage_start / z_sched_usage_stop decision flow
    let track_usage = true;
    let usage0: u32 = 1234;

    let start_d = start_decide(track_usage);
    assert_eq!(start_d, StartDecision::RecordStart);

    // After start, usage0 is set to non-zero
    let stop_d = stop_decide(usage0);
    assert_eq!(stop_d, StopDecision::Accumulate);

    // After stop, usage0 is cleared to 0
    let stop_d2 = stop_decide(0);
    assert_eq!(stop_d2, StopDecision::Skip);
}

#[test]
fn scenario_sys_enable_disable() {
    // Simulates k_sys_runtime_stats_enable/disable with idempotency
    let mut tracking = false;

    // Enable
    let d = sys_enable_decide(tracking);
    assert_eq!(d, SysTrackDecision::Apply);
    tracking = true;

    // Enable again — no-op
    let d2 = sys_enable_decide(tracking);
    assert_eq!(d2, SysTrackDecision::NoOp);

    // Disable
    let d3 = sys_disable_decide(tracking);
    assert_eq!(d3, SysTrackDecision::Apply);
    tracking = false;

    // Disable again — no-op
    let d4 = sys_disable_decide(tracking);
    assert_eq!(d4, SysTrackDecision::NoOp);
}
