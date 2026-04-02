//! Differential equivalence tests — Timer (FFI vs Model).
//!
//! Verifies that the FFI timer functions produce the same results as
//! the Verus-verified model functions in gale::timer.

#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use gale::error::*;
use gale::timer::Timer;

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Replica of gale_timer_init_validate.
fn ffi_timer_init_validate(period: u32) -> i32 {
    let _ = period;
    OK
}

/// Replica of gale_timer_expire.
/// Returns (ret, new_status).
fn ffi_timer_expire(status: u32) -> (i32, u32) {
    if status == u32::MAX {
        (EOVERFLOW, status)
    } else {
        (OK, status + 1)
    }
}

/// Replica of gale_timer_status_get.
/// Returns (old_status, new_status).
fn ffi_timer_status_get(status: u32) -> (u32, u32) {
    (status, 0)
}

/// Replica of gale_k_timer_expire_decide.
/// Returns (new_status, is_periodic).
fn ffi_timer_expire_decide(status: u32, period: u32) -> (u32, u8) {
    let new_status = if status < u32::MAX { status + 1 } else { status };
    let is_periodic = if period > 0 { 1u8 } else { 0u8 };
    (new_status, is_periodic)
}

/// Replica of gale_k_timer_status_decide.
/// Returns (count, new_status).
fn ffi_timer_status_decide(status: u32) -> (u32, u32) {
    (status, 0)
}

// =====================================================================
// Differential tests: timer_expire
// =====================================================================

#[test]
fn timer_expire_ffi_matches_model_exhaustive() {
    // Test small values and boundary
    let test_values: Vec<u32> = (0u32..=100)
        .chain(core::iter::once(u32::MAX - 1))
        .chain(core::iter::once(u32::MAX))
        .collect();

    for &status in &test_values {
        let (ffi_ret, ffi_new) = ffi_timer_expire(status);

        let mut t = Timer::init(100);
        t.status = status;
        t.running = true;
        let model_result = t.expire();

        match model_result {
            Ok(new_status) => {
                assert_eq!(ffi_ret, OK,
                    "ret mismatch at status={status}");
                assert_eq!(ffi_new, new_status,
                    "new_status mismatch at status={status}");
                assert_eq!(ffi_new, t.status,
                    "model status mismatch at status={status}");
            }
            Err(e) => {
                assert_eq!(ffi_ret, e,
                    "error mismatch at status={status}");
                // On overflow, status unchanged
                assert_eq!(t.status, status,
                    "status should be unchanged on overflow");
            }
        }
    }
}

// =====================================================================
// Differential tests: timer_status_get
// =====================================================================

#[test]
fn timer_status_get_ffi_matches_model_exhaustive() {
    let test_values: Vec<u32> = (0u32..=50)
        .chain(core::iter::once(u32::MAX))
        .collect();

    for &status in &test_values {
        let (ffi_old, ffi_new) = ffi_timer_status_get(status);

        let mut t = Timer::init(100);
        t.status = status;
        let model_old = t.status_get();

        assert_eq!(ffi_old, model_old,
            "old status mismatch at status={status}");
        assert_eq!(ffi_new, 0,
            "FFI new_status should be 0 at status={status}");
        assert_eq!(t.status, 0,
            "model status should be 0 after status_get at status={status}");
    }
}

// =====================================================================
// Differential tests: timer_expire_decide
// =====================================================================

#[test]
fn timer_expire_decide_ffi_matches_model() {
    let status_values: Vec<u32> = (0u32..=50)
        .chain(core::iter::once(u32::MAX - 1))
        .chain(core::iter::once(u32::MAX))
        .collect();

    for &status in &status_values {
        for period in [0u32, 1, 100, u32::MAX] {
            let (ffi_new_status, ffi_is_periodic) =
                ffi_timer_expire_decide(status, period);

            // Model behavior
            let mut t = Timer::init(period);
            t.status = status;
            let model_expire = t.expire();

            match model_expire {
                Ok(new_status) => {
                    assert_eq!(ffi_new_status, new_status,
                        "new_status mismatch: status={status}, period={period}");
                }
                Err(_) => {
                    // Overflow: FFI saturates at u32::MAX
                    assert_eq!(ffi_new_status, u32::MAX,
                        "should saturate at u32::MAX: status={status}");
                }
            }

            // Period classification
            let expected_periodic = if period > 0 { 1u8 } else { 0u8 };
            assert_eq!(ffi_is_periodic, expected_periodic,
                "is_periodic mismatch: period={period}");
        }
    }
}

// =====================================================================
// Differential tests: timer_status_decide
// =====================================================================

#[test]
fn timer_status_decide_ffi_matches_model() {
    let test_values: Vec<u32> = (0u32..=50)
        .chain(core::iter::once(u32::MAX))
        .collect();

    for &status in &test_values {
        let (ffi_count, ffi_new) = ffi_timer_status_decide(status);

        let mut t = Timer::init(0);
        t.status = status;
        let model_old = t.status_get();

        assert_eq!(ffi_count, model_old,
            "count mismatch at status={status}");
        assert_eq!(ffi_new, 0,
            "new_status should be 0 at status={status}");
        assert_eq!(t.status, 0,
            "model status should be reset at status={status}");
    }
}

// =====================================================================
// Differential tests: timer_init_validate
// =====================================================================

#[test]
fn timer_init_validate_always_ok() {
    for period in [0u32, 1, 100, u32::MAX] {
        let ret = ffi_timer_init_validate(period);
        assert_eq!(ret, OK, "init should always succeed for period={period}");

        // Model also accepts all periods
        let t = Timer::init(period);
        assert_eq!(t.period, period);
        assert_eq!(t.status, 0);
        assert!(!t.is_running());
    }
}

// =====================================================================
// Property: TM5 — expire increments status by exactly 1
// =====================================================================

#[test]
fn timer_expire_increments_by_one() {
    for status in 0u32..=200 {
        let (ffi_ret, ffi_new) = ffi_timer_expire(status);

        let mut t = Timer::init(0);
        t.status = status;
        let model_result = t.expire();

        assert_eq!(ffi_ret, OK);
        assert_eq!(ffi_new, status + 1, "TM5: must increment by exactly 1");
        assert_eq!(model_result.unwrap(), status + 1);
    }
}

// =====================================================================
// Property: TM8 — no overflow (saturate or error at u32::MAX)
// =====================================================================

#[test]
fn timer_expire_overflow_protection() {
    let (ffi_ret, ffi_new) = ffi_timer_expire(u32::MAX);
    assert_eq!(ffi_ret, EOVERFLOW, "TM8: must return EOVERFLOW at MAX");
    assert_eq!(ffi_new, u32::MAX, "TM8: status unchanged on overflow");

    let mut t = Timer::init(0);
    t.status = u32::MAX;
    let model_result = t.expire();
    assert!(model_result.is_err(), "TM8: model must also error");
    assert_eq!(t.status, u32::MAX, "TM8: model status unchanged");
}

// =====================================================================
// Property: TM2 — status_get reads and resets
// =====================================================================

#[test]
fn timer_status_get_read_and_reset() {
    for initial in [0u32, 1, 42, u32::MAX] {
        let (ffi_old, ffi_new) = ffi_timer_status_get(initial);
        assert_eq!(ffi_old, initial, "TM2: must return old value");
        assert_eq!(ffi_new, 0, "TM2: must reset to 0");

        let mut t = Timer::init(0);
        t.status = initial;
        let model_old = t.status_get();
        assert_eq!(model_old, initial);
        assert_eq!(t.status, 0);
    }
}

// =====================================================================
// Random operations: FFI matches model through a sequence
// =====================================================================

#[test]
fn timer_random_ops_ffi_matches_model() {
    let mut model = Timer::init(50);
    model.start();
    let mut ffi_status: u32 = 0;

    let mut rng: u32 = 0xBAAD_F00D;
    for _ in 0..1000 {
        rng = rng.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        match rng % 3 {
            0 => {
                // Expire
                let (ffi_ret, ffi_new) = ffi_timer_expire(ffi_status);
                let model_result = model.expire();
                match model_result {
                    Ok(_) => {
                        assert_eq!(ffi_ret, OK);
                        ffi_status = ffi_new;
                    }
                    Err(e) => {
                        assert_eq!(ffi_ret, e);
                    }
                }
            }
            1 => {
                // Status get
                let (ffi_old, ffi_new) = ffi_timer_status_get(ffi_status);
                let model_old = model.status_get();
                assert_eq!(ffi_old, model_old);
                ffi_status = ffi_new;
            }
            _ => {
                // Start (reset)
                ffi_status = 0;
                model.start();
            }
        }
        assert_eq!(ffi_status, model.status_peek(),
            "FFI/model timer status diverged at rng={rng}");
    }
}
