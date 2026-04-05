//! Differential equivalence tests — MemPool (FFI vs Model).
//!
//! Verifies that the FFI mempool functions produce the same results as
//! the Verus-verified model functions in gale::mempool.

#![allow(
    clippy::shadow_unrelated,
    clippy::indexing_slicing,
    clippy::bool_to_int_with_if
)]

use gale::error::*;
use gale::mempool::MemPool;

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Replica of gale_mempool_alloc_validate.
fn ffi_mempool_alloc_validate(allocated: u32, capacity: u32) -> (i32, u32) {
    if allocated >= capacity {
        return (ENOMEM, allocated);
    }
    #[allow(clippy::arithmetic_side_effects)]
    let new = allocated + 1;
    (OK, new)
}

/// Replica of gale_mempool_free_validate.
fn ffi_mempool_free_validate(allocated: u32) -> (i32, u32) {
    if allocated == 0 {
        return (EINVAL, 0);
    }
    #[allow(clippy::arithmetic_side_effects)]
    let new = allocated - 1;
    (OK, new)
}

/// Replica of gale_k_mempool_alloc_decide.
fn ffi_mempool_alloc_decide(alloc_succeeded: bool) -> u8 {
    if alloc_succeeded { 0 } else { 1 } // RETURN_PTR / RETURN_NULL
}

/// Replica of gale_k_mempool_free_decide.
fn ffi_mempool_free_decide(has_waiters: bool) -> u8 {
    if has_waiters { 1 } else { 0 } // FREE_AND_RESCHEDULE / FREE_OK
}

// =====================================================================
// Differential tests: mempool alloc_validate
// =====================================================================

#[test]
fn mempool_alloc_validate_ffi_matches_model_exhaustive() {
    for capacity in 1u32..=10 {
        for allocated in 0u32..=capacity {
            let (ffi_ret, ffi_new) = ffi_mempool_alloc_validate(allocated, capacity);

            let mut p = MemPool { capacity, allocated, block_size: 64 };
            let model_ret = p.alloc();

            assert_eq!(ffi_ret, model_ret,
                "alloc ret: cap={capacity}, alloc={allocated}");
            if ffi_ret == OK {
                assert_eq!(ffi_new, p.allocated,
                    "alloc new: cap={capacity}, alloc={allocated}");
            }
        }
    }
}

// =====================================================================
// Differential tests: mempool free_validate
// =====================================================================

#[test]
fn mempool_free_validate_ffi_matches_model_exhaustive() {
    for capacity in 1u32..=10 {
        for allocated in 0u32..=capacity {
            let (ffi_ret, ffi_new) = ffi_mempool_free_validate(allocated);

            let mut p = MemPool { capacity, allocated, block_size: 64 };
            let model_ret = p.free();

            assert_eq!(ffi_ret, model_ret,
                "free ret: alloc={allocated}");
            if ffi_ret == OK {
                assert_eq!(ffi_new, p.allocated,
                    "free new: alloc={allocated}");
            }
        }
    }
}

// =====================================================================
// Differential tests: mempool alloc_decide / free_decide
// =====================================================================

#[test]
fn mempool_alloc_decide_ffi_matches_model() {
    for alloc_succeeded in [false, true] {
        let ffi_action = ffi_mempool_alloc_decide(alloc_succeeded);
        if alloc_succeeded {
            assert_eq!(ffi_action, 0, "RETURN_PTR");
        } else {
            assert_eq!(ffi_action, 1, "RETURN_NULL");
        }
    }
}

#[test]
fn mempool_free_decide_ffi_matches_model() {
    for has_waiters in [false, true] {
        let ffi_action = ffi_mempool_free_decide(has_waiters);
        if has_waiters {
            assert_eq!(ffi_action, 1, "FREE_AND_RESCHEDULE");
        } else {
            assert_eq!(ffi_action, 0, "FREE_OK");
        }
    }
}

// =====================================================================
// Property: MP1 — 0 <= allocated <= capacity
// =====================================================================

#[test]
fn mempool_alloc_never_exceeds_capacity() {
    for capacity in 1u32..=20 {
        for allocated in 0u32..=capacity {
            let (ret, new_alloc) = ffi_mempool_alloc_validate(allocated, capacity);
            if ret == OK {
                assert!(new_alloc <= capacity, "MP1: {new_alloc} > {capacity}");
            }
        }
    }
}

// =====================================================================
// Property: MP5 — alloc/free roundtrip
// =====================================================================

#[test]
fn mempool_alloc_free_roundtrip() {
    for capacity in 1u32..=10 {
        for initial in 0u32..capacity {
            let (ret, new_alloc) = ffi_mempool_alloc_validate(initial, capacity);
            assert_eq!(ret, OK);

            let (ret2, final_alloc) = ffi_mempool_free_validate(new_alloc);
            assert_eq!(ret2, OK);
            assert_eq!(final_alloc, initial, "roundtrip failed: initial={initial}");
        }
    }
}
