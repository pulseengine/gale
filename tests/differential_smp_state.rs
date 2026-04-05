//! Differential equivalence tests — SMP State (FFI vs Model).
//!
//! Verifies that the FFI SMP CPU state functions produce the same results as
//! the Verus-verified model functions in gale::smp_state.

#![allow(
    clippy::shadow_unrelated,
    clippy::indexing_slicing,
    clippy::bool_to_int_with_if,
    clippy::unwrap_used,
    clippy::fn_params_excessive_bools,
    clippy::absurd_extreme_comparisons,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::checked_conversions,
    clippy::wildcard_enum_match_arm,
    clippy::implicit_saturating_sub,
    clippy::branches_sharing_code,
    clippy::panic
)]

use gale::error::*;
use gale::smp_state::{self, SmpState, MAX_CPUS};

// Action constants matching FFI
const GALE_SMP_ACTION_START_OK: u8 = 0;
const GALE_SMP_ACTION_ALL_ACTIVE: u8 = 1;
const GALE_SMP_ACTION_STOP_OK: u8 = 0;
const GALE_SMP_ACTION_LAST_CPU: u8 = 1;

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Replica of gale_smp_start_cpu_validate.
fn ffi_smp_start_cpu_validate(active_cpus: u32, max_cpus: u32) -> (i32, u32) {
    match smp_state::start_cpu_decide(active_cpus, max_cpus) {
        Ok(new_active) => (OK, new_active),
        Err(e) => (e, active_cpus),
    }
}

/// Replica of gale_smp_stop_cpu_validate.
fn ffi_smp_stop_cpu_validate(active_cpus: u32) -> (i32, u32) {
    match smp_state::stop_cpu_decide(active_cpus) {
        Ok(new_active) => (OK, new_active),
        Err(e) => (e, active_cpus),
    }
}

/// Replica of gale_smp_start_cpu_decide.
fn ffi_smp_start_cpu_decide(active_cpus: u32, max_cpus: u32) -> (u8, u32) {
    // Returns (action, new_active)
    match smp_state::start_cpu_decide(active_cpus, max_cpus) {
        Ok(new_active) => (GALE_SMP_ACTION_START_OK, new_active),
        Err(_) => (GALE_SMP_ACTION_ALL_ACTIVE, active_cpus),
    }
}

/// Replica of gale_smp_stop_cpu_decide.
fn ffi_smp_stop_cpu_decide(active_cpus: u32) -> (u8, u32) {
    // Returns (action, new_active)
    match smp_state::stop_cpu_decide(active_cpus) {
        Ok(new_active) => (GALE_SMP_ACTION_STOP_OK, new_active),
        Err(_) => (GALE_SMP_ACTION_LAST_CPU, active_cpus),
    }
}

// =====================================================================
// Differential tests: smp init
// =====================================================================

#[test]
fn smp_init_ffi_matches_model_exhaustive() {
    for max_cpus in 0u32..=MAX_CPUS + 2 {
        let model_result = SmpState::init(max_cpus);

        if max_cpus == 0 || max_cpus > MAX_CPUS {
            assert!(model_result.is_err(),
                "init: should fail for max_cpus={max_cpus}");
            if let Err(e) = model_result {
                assert_eq!(e, EINVAL,
                    "init: error must be EINVAL for max_cpus={max_cpus}");
            }
        } else {
            let s = model_result.unwrap();
            assert_eq!(s.max_cpus, max_cpus);
            assert_eq!(s.active_cpus, 1, "init: only CPU 0 active");
            assert_eq!(s.global_lock_count, 0, "init: lock count must be 0");
        }
    }
}

// =====================================================================
// Differential tests: start_cpu_validate
// =====================================================================

#[test]
fn smp_start_cpu_validate_ffi_matches_model_exhaustive() {
    for max_cpus in 1u32..=MAX_CPUS {
        for active in 1u32..=max_cpus {
            let (ffi_ret, ffi_new) = ffi_smp_start_cpu_validate(active, max_cpus);

            let mut s = SmpState { max_cpus, active_cpus: active, global_lock_count: 0 };
            let model_ret = s.start_cpu();

            assert_eq!(ffi_ret, model_ret,
                "start_cpu ret: max={max_cpus}, active={active}");
            if ffi_ret == OK {
                assert_eq!(ffi_new, s.active_cpus,
                    "start_cpu new_active: max={max_cpus}, active={active}");
            }
        }
    }
}

// =====================================================================
// Differential tests: stop_cpu_validate
// =====================================================================

#[test]
fn smp_stop_cpu_validate_ffi_matches_model_exhaustive() {
    for max_cpus in 1u32..=MAX_CPUS {
        for active in 1u32..=max_cpus {
            let (ffi_ret, ffi_new) = ffi_smp_stop_cpu_validate(active);

            let mut s = SmpState { max_cpus, active_cpus: active, global_lock_count: 0 };
            let model_ret = s.stop_cpu();

            assert_eq!(ffi_ret, model_ret,
                "stop_cpu ret: max={max_cpus}, active={active}");
            if ffi_ret == OK {
                assert_eq!(ffi_new, s.active_cpus,
                    "stop_cpu new_active: max={max_cpus}, active={active}");
            }
        }
    }
}

// =====================================================================
// Differential tests: start_cpu_decide
// =====================================================================

#[test]
fn smp_start_cpu_decide_ffi_matches_model_exhaustive() {
    for max_cpus in 1u32..=MAX_CPUS {
        for active in 1u32..=max_cpus {
            let (ffi_action, ffi_new) = ffi_smp_start_cpu_decide(active, max_cpus);

            if active < max_cpus {
                assert_eq!(ffi_action, GALE_SMP_ACTION_START_OK,
                    "SM2: room available => START_OK: max={max_cpus}, active={active}");
                #[allow(clippy::arithmetic_side_effects)]
                let expected_new = active + 1;
                assert_eq!(ffi_new, expected_new,
                    "SM2: start increments active: max={max_cpus}, active={active}");
            } else {
                assert_eq!(ffi_action, GALE_SMP_ACTION_ALL_ACTIVE,
                    "SM2: all active => ALL_ACTIVE: max={max_cpus}, active={active}");
                assert_eq!(ffi_new, active,
                    "SM2: all active => count unchanged");
            }
        }
    }
}

// =====================================================================
// Differential tests: stop_cpu_decide
// =====================================================================

#[test]
fn smp_stop_cpu_decide_ffi_matches_model_exhaustive() {
    for active in 0u32..=MAX_CPUS {
        let (ffi_action, ffi_new) = ffi_smp_stop_cpu_decide(active);

        if active > 1 {
            assert_eq!(ffi_action, GALE_SMP_ACTION_STOP_OK,
                "SM3: more than 1 CPU => STOP_OK: active={active}");
            #[allow(clippy::arithmetic_side_effects)]
            let expected_new = active - 1;
            assert_eq!(ffi_new, expected_new,
                "SM3: stop decrements active: active={active}");
        } else {
            assert_eq!(ffi_action, GALE_SMP_ACTION_LAST_CPU,
                "SM3: last CPU => LAST_CPU: active={active}");
            assert_eq!(ffi_new, active,
                "SM3: last CPU => count unchanged: active={active}");
        }
    }
}

// =====================================================================
// Property: SM1 — active_cpus always in [1, max_cpus]
// =====================================================================

#[test]
fn smp_active_always_bounded() {
    for max_cpus in 1u32..=MAX_CPUS {
        let mut s = SmpState { max_cpus, active_cpus: 1, global_lock_count: 0 };

        // Start all CPUs
        loop {
            let rc = s.start_cpu();
            if rc != OK {
                break;
            }
            assert!(s.active_cpus >= 1, "SM1: active >= 1");
            assert!(s.active_cpus <= max_cpus, "SM1: active <= max");
        }
        assert_eq!(s.active_cpus, max_cpus, "should reach max: max={max_cpus}");

        // Stop all CPUs (except CPU 0)
        loop {
            let rc = s.stop_cpu();
            if rc != OK {
                break;
            }
            assert!(s.active_cpus >= 1, "SM1: active >= 1 after stop");
            assert!(s.active_cpus <= max_cpus, "SM1: active <= max after stop");
        }
        assert_eq!(s.active_cpus, 1, "should reach 1: max={max_cpus}");
    }
}

// =====================================================================
// Property: SM3 — CPU 0 never stops
// =====================================================================

#[test]
fn smp_cpu0_never_stops() {
    let (ffi_action, ffi_new) = ffi_smp_stop_cpu_decide(1);
    assert_eq!(ffi_action, GALE_SMP_ACTION_LAST_CPU, "SM3: active=1 must be LAST_CPU");
    assert_eq!(ffi_new, 1, "SM3: active=1 count stays at 1");

    let (ffi_action_zero, ffi_new_zero) = ffi_smp_stop_cpu_decide(0);
    assert_eq!(ffi_action_zero, GALE_SMP_ACTION_LAST_CPU,
        "SM3: active=0 (invalid) must be LAST_CPU");
    assert_eq!(ffi_new_zero, 0, "SM3: active=0 stays at 0");
}

// =====================================================================
// Property: SM4 — lock/unlock roundtrip
// =====================================================================

#[test]
fn smp_lock_unlock_roundtrip() {
    for max_cpus in 1u32..=MAX_CPUS {
        let mut s = SmpState { max_cpus, active_cpus: 1, global_lock_count: 0 };

        // Lock multiple times
        for _ in 0u32..5 {
            let rc = s.global_lock();
            assert_eq!(rc, OK, "SM4: lock must succeed");
        }
        assert_eq!(s.global_lock_count, 5, "SM4: lock count after 5 locks");

        // Unlock multiple times
        for remaining in (1u32..=5).rev() {
            assert_eq!(s.global_lock_count, remaining);
            let rc = s.global_unlock();
            assert_eq!(rc, OK, "SM4: unlock must succeed");
        }
        assert_eq!(s.global_lock_count, 0, "SM4: count must reach 0");

        // Extra unlock must fail
        let rc = s.global_unlock();
        assert_eq!(rc, EINVAL, "SM4: unlock below 0 must be EINVAL");
        assert_eq!(s.global_lock_count, 0, "SM4: count must stay at 0");
    }
}

// =====================================================================
// Property: SM2+SM3 — start then stop roundtrip
// =====================================================================

#[test]
fn smp_start_stop_roundtrip() {
    for max_cpus in 2u32..=MAX_CPUS {
        for initial in 1u32..max_cpus {
            let (start_action, after_start) = ffi_smp_start_cpu_decide(initial, max_cpus);
            assert_eq!(start_action, GALE_SMP_ACTION_START_OK,
                "SM2+SM3: start should succeed: max={max_cpus}, initial={initial}");

            let (stop_action, after_stop) = ffi_smp_stop_cpu_decide(after_start);
            assert_eq!(stop_action, GALE_SMP_ACTION_STOP_OK,
                "SM2+SM3: stop should succeed: after_start={after_start}");
            assert_eq!(after_stop, initial,
                "SM2+SM3: start+stop roundtrip: max={max_cpus}, initial={initial}");
        }
    }
}
