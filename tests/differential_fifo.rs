//! Differential equivalence tests — Fifo (FFI vs Model).
//!
//! Verifies that the FFI fifo functions produce the same results as
//! the Verus-verified model functions in gale::fifo.

#![allow(
    clippy::shadow_unrelated,
    clippy::indexing_slicing,
    clippy::bool_to_int_with_if
)]

use gale::error::*;
use gale::fifo::{self, Fifo, GetDecision, PutDecision};

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Replica of gale_fifo_put_validate (ffi/src/lib.rs).
fn ffi_fifo_put_validate(count: u32) -> (i32, u32) {
    if count >= u32::MAX - 1 {
        return (EOVERFLOW, count);
    }
    #[allow(clippy::arithmetic_side_effects)]
    let new_count = count + 1;
    (OK, new_count)
}

/// Replica of gale_fifo_get_validate (ffi/src/lib.rs).
fn ffi_fifo_get_validate(count: u32) -> (i32, u32) {
    if count == 0 {
        return (EAGAIN, 0);
    }
    #[allow(clippy::arithmetic_side_effects)]
    let new_count = count - 1;
    (OK, new_count)
}

/// Replica of gale_k_fifo_put_decide (ffi/src/lib.rs).
fn ffi_fifo_put_decide(has_waiter: bool) -> u8 {
    if has_waiter { 1 } else { 0 }
}

/// Replica of gale_k_fifo_get_decide (ffi/src/lib.rs).
fn ffi_fifo_get_decide(count: u32, is_no_wait: bool) -> (i32, u8) {
    if count > 0 {
        (OK, 0) // GET_OK
    } else if is_no_wait {
        (EBUSY, 2) // NODATA
    } else {
        (0, 1) // PEND
    }
}

// =====================================================================
// Differential tests: fifo put_validate
// =====================================================================

#[test]
fn fifo_put_validate_ffi_matches_model_exhaustive() {
    for count in 0u32..=20 {
        let (ffi_ret, ffi_new) = ffi_fifo_put_validate(count);

        let mut f = Fifo { count };
        let model_ret = f.put();

        assert_eq!(ffi_ret, model_ret,
            "put ret mismatch: count={count}");
        if ffi_ret == OK {
            assert_eq!(ffi_new, f.count,
                "put new_count mismatch: count={count}");
        }
    }
}

#[test]
fn fifo_put_validate_overflow_boundary() {
    let (ret, _) = ffi_fifo_put_validate(u32::MAX - 1);
    assert_eq!(ret, EOVERFLOW);

    let (ret, _) = ffi_fifo_put_validate(u32::MAX);
    assert_eq!(ret, EOVERFLOW);
}

#[test]
fn fifo_get_validate_ffi_matches_model_exhaustive() {
    for count in 0u32..=20 {
        let (ffi_ret, ffi_new) = ffi_fifo_get_validate(count);

        let mut f = Fifo { count };
        let model_ret = f.get();

        assert_eq!(ffi_ret, model_ret,
            "get ret mismatch: count={count}");
        if ffi_ret == OK {
            assert_eq!(ffi_new, f.count,
                "get new_count mismatch: count={count}");
        }
    }
}

// =====================================================================
// Differential tests: fifo put_decide / get_decide
// =====================================================================

#[test]
fn fifo_put_decide_ffi_matches_model() {
    for count in 0u32..=10 {
        for has_waiter in [false, true] {
            let ffi_action = ffi_fifo_put_decide(has_waiter);
            let model = fifo::put_decide(count, has_waiter);

            let expected_action = match model {
                PutDecision::WakeThread => 1u8,
                PutDecision::Insert => 0,
                PutDecision::Overflow => {
                    // FFI put_decide doesn't handle overflow (unbounded)
                    continue;
                }
            };
            assert_eq!(ffi_action, expected_action,
                "put_decide action mismatch: count={count}, has_waiter={has_waiter}");
        }
    }
}

#[test]
fn fifo_get_decide_ffi_matches_model() {
    for count in 0u32..=10 {
        for is_no_wait in [false, true] {
            let (ffi_ret, ffi_action) = ffi_fifo_get_decide(count, is_no_wait);
            let model = fifo::get_decide(count);

            match model {
                GetDecision::Dequeued => {
                    assert_eq!(ffi_action, 0, "get_decide action: count={count}");
                    assert_eq!(ffi_ret, OK, "get_decide ret: count={count}");
                }
                GetDecision::Empty => {
                    if is_no_wait {
                        assert_eq!(ffi_action, 2, "get_decide nodata: count={count}");
                        assert_eq!(ffi_ret, EBUSY, "get_decide ret: count={count}");
                    } else {
                        assert_eq!(ffi_action, 1, "get_decide pend: count={count}");
                    }
                }
            }
        }
    }
}

// =====================================================================
// Property: FI2+FI3 — put then get roundtrip
// =====================================================================

#[test]
fn fifo_put_get_roundtrip() {
    for initial in 0u32..=50 {
        let mut f = Fifo { count: initial };
        if f.put() == OK {
            let rc = f.get();
            assert_eq!(rc, OK);
            assert_eq!(f.count, initial, "roundtrip failed: initial={initial}");
        }
    }
}
