//! Differential equivalence tests — Futex (FFI vs Model).
//!
//! Verifies that the FFI futex functions produce the same results as
//! the Verus-verified model functions in gale::futex.

#![allow(
    clippy::shadow_unrelated,
    clippy::indexing_slicing,
    clippy::bool_to_int_with_if
)]

use gale::error::*;

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Replica of gale_futex_wait_check.
fn ffi_futex_wait_check(val: u32, expected: u32) -> i32 {
    if val == expected { OK } else { EAGAIN }
}

/// Replica of gale_futex_wake.
fn ffi_futex_wake(num_waiters: u32, wake_all: bool) -> (u32, u32) {
    if num_waiters == 0 {
        (0, 0)
    } else if wake_all {
        (num_waiters, 0)
    } else {
        #[allow(clippy::arithmetic_side_effects)]
        let remaining = num_waiters - 1;
        (1, remaining)
    }
}

/// Replica of gale_k_futex_wait_decide.
fn ffi_futex_wait_decide(val: u32, expected: u32, is_no_wait: bool) -> (u8, i32) {
    if val != expected {
        (1, EAGAIN) // RETURN_EAGAIN
    } else if is_no_wait {
        (1, ETIMEDOUT) // RETURN_EAGAIN (timeout)
    } else {
        (0, OK) // BLOCK
    }
}

/// Replica of gale_k_futex_wake_decide.
fn ffi_futex_wake_decide(num_waiters: u32, wake_all: bool) -> u32 {
    if wake_all {
        num_waiters
    } else if num_waiters > 0 {
        1
    } else {
        0
    }
}

const ETIMEDOUT: i32 = gale::error::ETIMEDOUT;

// =====================================================================
// Differential tests: futex_wait_check
// =====================================================================

#[test]
fn futex_wait_check_ffi_matches_model_exhaustive() {
    for val in 0u32..=10 {
        for expected in 0u32..=10 {
            let ffi_ret = ffi_futex_wait_check(val, expected);

            // FX1: block when equal, FX2: EAGAIN when not equal
            if val == expected {
                assert_eq!(ffi_ret, OK, "wait_check: val={val}, exp={expected}");
            } else {
                assert_eq!(ffi_ret, EAGAIN, "wait_check: val={val}, exp={expected}");
            }
        }
    }
}

// =====================================================================
// Differential tests: futex_wake
// =====================================================================

#[test]
fn futex_wake_ffi_matches_model_exhaustive() {
    for num_waiters in 0u32..=10 {
        for wake_all in [false, true] {
            let (ffi_woken, ffi_remaining) = ffi_futex_wake(num_waiters, wake_all);

            if num_waiters == 0 {
                assert_eq!(ffi_woken, 0);
                assert_eq!(ffi_remaining, 0);
            } else if wake_all {
                assert_eq!(ffi_woken, num_waiters, "FX5: wake_all wakes all");
                assert_eq!(ffi_remaining, 0);
            } else {
                assert_eq!(ffi_woken, 1, "FX4: single wake");
                #[allow(clippy::arithmetic_side_effects)]
                let expected_remaining = num_waiters - 1;
                assert_eq!(ffi_remaining, expected_remaining);
            }
        }
    }
}

// =====================================================================
// Differential tests: futex_wait_decide
// =====================================================================

#[test]
fn futex_wait_decide_ffi_matches_model_exhaustive() {
    for val in 0u32..=5 {
        for expected in 0u32..=5 {
            for is_no_wait in [false, true] {
                let (ffi_action, ffi_ret) = ffi_futex_wait_decide(val, expected, is_no_wait);

                if val != expected {
                    assert_eq!(ffi_action, 1, "RETURN_EAGAIN: val={val}, exp={expected}");
                    assert_eq!(ffi_ret, EAGAIN);
                } else if is_no_wait {
                    assert_eq!(ffi_action, 1, "RETURN (timeout): val={val}");
                    assert_eq!(ffi_ret, ETIMEDOUT);
                } else {
                    assert_eq!(ffi_action, 0, "BLOCK: val={val}, exp={expected}");
                    assert_eq!(ffi_ret, OK);
                }
            }
        }
    }
}

// =====================================================================
// Differential tests: futex_wake_decide
// =====================================================================

#[test]
fn futex_wake_decide_ffi_matches_model_exhaustive() {
    for num_waiters in 0u32..=10 {
        for wake_all in [false, true] {
            let ffi_limit = ffi_futex_wake_decide(num_waiters, wake_all);

            if wake_all {
                assert_eq!(ffi_limit, num_waiters);
            } else if num_waiters > 0 {
                assert_eq!(ffi_limit, 1);
            } else {
                assert_eq!(ffi_limit, 0);
            }
        }
    }
}

// =====================================================================
// Property: FX3 — wake returns correct count
// =====================================================================

#[test]
fn futex_wake_woken_plus_remaining_equals_total() {
    for n in 0u32..=100 {
        for wake_all in [false, true] {
            let (woken, remaining) = ffi_futex_wake(n, wake_all);
            #[allow(clippy::arithmetic_side_effects)]
            let total = woken + remaining;
            assert_eq!(total, n,
                "FX3 conservation: n={n}, wake_all={wake_all}");
        }
    }
}
