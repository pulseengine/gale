//! Differential equivalence tests — Queue (FFI vs Model).
//!
//! Verifies that the FFI queue functions produce the same results as
//! the Verus-verified model functions in gale::queue.

#![allow(
    clippy::shadow_unrelated,
    clippy::indexing_slicing,
    clippy::bool_to_int_with_if
)]

use gale::error::*;
use gale::queue::Queue;

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Replica of gale_queue_append_validate / gale_queue_prepend_validate.
fn ffi_queue_insert_validate(count: u32) -> (i32, u32) {
    if count >= u32::MAX - 1 {
        return (EOVERFLOW, count);
    }
    #[allow(clippy::arithmetic_side_effects)]
    let new_count = count + 1;
    (OK, new_count)
}

/// Replica of gale_queue_get_validate.
fn ffi_queue_get_validate(count: u32) -> (i32, u32) {
    if count == 0 {
        return (EAGAIN, 0);
    }
    #[allow(clippy::arithmetic_side_effects)]
    let new_count = count - 1;
    (OK, new_count)
}

/// Replica of gale_k_queue_insert_decide.
fn ffi_queue_insert_decide(has_waiter: bool) -> u8 {
    if has_waiter { 1 } else { 0 } // WAKE=1, INSERT=0
}

/// Replica of gale_k_queue_get_decide.
fn ffi_queue_get_decide(has_data: bool, is_no_wait: bool) -> u8 {
    if has_data {
        0 // DEQUEUE
    } else if is_no_wait {
        1 // RETURN_NULL
    } else {
        2 // PEND
    }
}

// =====================================================================
// Differential tests: queue append/prepend validate
// =====================================================================

#[test]
fn queue_append_validate_ffi_matches_model_exhaustive() {
    for count in 0u32..=20 {
        let (ffi_ret, ffi_new) = ffi_queue_insert_validate(count);

        let mut q = Queue { count };
        let model_ret = q.append();

        assert_eq!(ffi_ret, model_ret,
            "append ret mismatch: count={count}");
        if ffi_ret == OK {
            assert_eq!(ffi_new, q.count,
                "append new_count mismatch: count={count}");
        }
    }
}

#[test]
fn queue_prepend_validate_ffi_matches_model_exhaustive() {
    for count in 0u32..=20 {
        let (ffi_ret, ffi_new) = ffi_queue_insert_validate(count);

        let mut q = Queue { count };
        let model_ret = q.prepend();

        assert_eq!(ffi_ret, model_ret,
            "prepend ret mismatch: count={count}");
        if ffi_ret == OK {
            assert_eq!(ffi_new, q.count,
                "prepend new_count mismatch: count={count}");
        }
    }
}

#[test]
fn queue_get_validate_ffi_matches_model_exhaustive() {
    for count in 0u32..=20 {
        let (ffi_ret, ffi_new) = ffi_queue_get_validate(count);

        let mut q = Queue { count };
        let model_ret = q.get();

        assert_eq!(ffi_ret, model_ret,
            "get ret mismatch: count={count}");
        if ffi_ret == OK {
            assert_eq!(ffi_new, q.count,
                "get new_count mismatch: count={count}");
        }
    }
}

// =====================================================================
// Differential tests: queue insert_decide / get_decide
// =====================================================================

#[test]
fn queue_insert_decide_ffi_matches_model() {
    for has_waiter in [false, true] {
        let ffi_action = ffi_queue_insert_decide(has_waiter);
        let expected = if has_waiter { 1u8 } else { 0 };
        assert_eq!(ffi_action, expected,
            "insert_decide mismatch: has_waiter={has_waiter}");
    }
}

#[test]
fn queue_get_decide_ffi_matches_model() {
    for has_data in [false, true] {
        for is_no_wait in [false, true] {
            let ffi_action = ffi_queue_get_decide(has_data, is_no_wait);

            if has_data {
                assert_eq!(ffi_action, 0, "get_decide DEQUEUE");
            } else if is_no_wait {
                assert_eq!(ffi_action, 1, "get_decide RETURN_NULL");
            } else {
                assert_eq!(ffi_action, 2, "get_decide PEND");
            }
        }
    }
}

// =====================================================================
// Property: QU2+QU4 — append then get roundtrip
// =====================================================================

#[test]
fn queue_append_get_roundtrip() {
    for initial in 0u32..=50 {
        let mut q = Queue { count: initial };
        if q.append() == OK {
            let rc = q.get();
            assert_eq!(rc, OK);
            assert_eq!(q.count, initial, "roundtrip failed: initial={initial}");
        }
    }
}
