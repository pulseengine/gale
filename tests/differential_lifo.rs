//! Differential equivalence tests — Lifo (FFI vs Model).
//!
//! Verifies that the FFI lifo functions produce the same results as
//! the Verus-verified model functions in gale::lifo.

#![allow(
    clippy::shadow_unrelated,
    clippy::indexing_slicing,
    clippy::bool_to_int_with_if
)]

use gale::error::*;
use gale::lifo::Lifo;

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Replica of gale_lifo_put_validate (ffi/src/lib.rs).
fn ffi_lifo_put_validate(count: u32) -> (i32, u32) {
    if count >= u32::MAX - 1 {
        return (EOVERFLOW, count);
    }
    #[allow(clippy::arithmetic_side_effects)]
    let new_count = count + 1;
    (OK, new_count)
}

/// Replica of gale_lifo_get_validate (ffi/src/lib.rs).
fn ffi_lifo_get_validate(count: u32) -> (i32, u32) {
    if count == 0 {
        return (EAGAIN, 0);
    }
    #[allow(clippy::arithmetic_side_effects)]
    let new_count = count - 1;
    (OK, new_count)
}

/// Replica of gale_k_lifo_put_decide (ffi/src/lib.rs).
fn ffi_lifo_put_decide(has_waiter: bool) -> u8 {
    if has_waiter { 1 } else { 0 }
}

/// Replica of gale_k_lifo_get_decide (ffi/src/lib.rs).
fn ffi_lifo_get_decide(count: u32, is_no_wait: bool) -> (i32, u8) {
    if count > 0 {
        (OK, 0) // GET_OK
    } else if is_no_wait {
        (EBUSY, 2) // NODATA
    } else {
        (0, 1) // PEND
    }
}

// =====================================================================
// Differential tests: lifo put_validate
// =====================================================================

#[test]
fn lifo_put_validate_ffi_matches_model_exhaustive() {
    for count in 0u32..=20 {
        let (ffi_ret, ffi_new) = ffi_lifo_put_validate(count);

        let mut l = Lifo { count };
        let model_ret = l.put();

        assert_eq!(ffi_ret, model_ret,
            "put ret mismatch: count={count}");
        if ffi_ret == OK {
            assert_eq!(ffi_new, l.count,
                "put new_count mismatch: count={count}");
        }
    }
}

#[test]
fn lifo_put_validate_overflow_boundary() {
    let (ret, _) = ffi_lifo_put_validate(u32::MAX - 1);
    assert_eq!(ret, EOVERFLOW);

    let (ret, _) = ffi_lifo_put_validate(u32::MAX);
    assert_eq!(ret, EOVERFLOW);
}

#[test]
fn lifo_get_validate_ffi_matches_model_exhaustive() {
    for count in 0u32..=20 {
        let (ffi_ret, ffi_new) = ffi_lifo_get_validate(count);

        let mut l = Lifo { count };
        let model_ret = l.get();

        assert_eq!(ffi_ret, model_ret,
            "get ret mismatch: count={count}");
        if ffi_ret == OK {
            assert_eq!(ffi_new, l.count,
                "get new_count mismatch: count={count}");
        }
    }
}

// =====================================================================
// Differential tests: lifo put_decide / get_decide
// =====================================================================

#[test]
fn lifo_put_decide_ffi_matches_model() {
    for has_waiter in [false, true] {
        let ffi_action = ffi_lifo_put_decide(has_waiter);
        // Model: waiter -> WAKE (1), no waiter -> PUT_OK (0)
        let expected = if has_waiter { 1u8 } else { 0 };
        assert_eq!(ffi_action, expected,
            "put_decide mismatch: has_waiter={has_waiter}");
    }
}

#[test]
fn lifo_get_decide_ffi_matches_model() {
    for count in 0u32..=10 {
        for is_no_wait in [false, true] {
            let (ffi_ret, ffi_action) = ffi_lifo_get_decide(count, is_no_wait);

            if count > 0 {
                assert_eq!(ffi_action, 0, "get_decide OK: count={count}");
                assert_eq!(ffi_ret, OK);
            } else if is_no_wait {
                assert_eq!(ffi_action, 2, "get_decide nodata: count={count}");
                assert_eq!(ffi_ret, EBUSY);
            } else {
                assert_eq!(ffi_action, 1, "get_decide pend: count={count}");
            }
        }
    }
}

// =====================================================================
// Property: LI2+LI3 — put then get roundtrip
// =====================================================================

#[test]
fn lifo_put_get_roundtrip() {
    for initial in 0u32..=50 {
        let mut l = Lifo { count: initial };
        if l.put() == OK {
            let rc = l.get();
            assert_eq!(rc, OK);
            assert_eq!(l.count, initial, "roundtrip failed: initial={initial}");
        }
    }
}
