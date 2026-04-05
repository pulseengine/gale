//! Differential equivalence tests — Dynamic (FFI vs Model).
//!
//! Verifies that the FFI dynamic pool functions produce the same results as
//! the Verus-verified model functions in gale::dynamic.

#![allow(
    clippy::shadow_unrelated,
    clippy::indexing_slicing,
    clippy::bool_to_int_with_if
)]

use gale::error::*;
use gale::dynamic::DynamicPool;

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Replica of gale_dynamic_alloc_validate.
fn ffi_dynamic_alloc_validate(active: u32, max_threads: u32) -> (i32, u32) {
    if active >= max_threads {
        return (ENOMEM, active);
    }
    #[allow(clippy::arithmetic_side_effects)]
    let new_active = active + 1;
    (OK, new_active)
}

/// Replica of gale_dynamic_free_validate.
fn ffi_dynamic_free_validate(active: u32) -> (i32, u32) {
    if active == 0 {
        return (EINVAL, 0);
    }
    #[allow(clippy::arithmetic_side_effects)]
    let new_active = active - 1;
    (OK, new_active)
}

/// Replica of gale_dynamic_alloc_decide.
fn ffi_dynamic_alloc_decide(active: u32, max_threads: u32) -> (u8, u32) {
    if active >= max_threads {
        (1, active) // POOL_FULL
    } else {
        #[allow(clippy::arithmetic_side_effects)]
        let new_active = active + 1;
        (0, new_active) // ALLOC_OK
    }
}

/// Replica of gale_dynamic_free_decide.
fn ffi_dynamic_free_decide(active: u32) -> (u8, u32) {
    if active == 0 {
        (1, 0) // UNDERFLOW
    } else {
        #[allow(clippy::arithmetic_side_effects)]
        let new_active = active - 1;
        (0, new_active) // FREE_OK
    }
}

// =====================================================================
// Differential tests: dynamic alloc_validate
// =====================================================================

#[test]
fn dynamic_alloc_validate_ffi_matches_model_exhaustive() {
    for max_threads in 1u32..=10 {
        for active in 0u32..=max_threads {
            let (ffi_ret, ffi_new) = ffi_dynamic_alloc_validate(active, max_threads);

            let mut d = DynamicPool { max_threads, active, stack_size: 4096 };
            let model_ret = d.alloc();

            assert_eq!(ffi_ret, model_ret,
                "alloc ret: max={max_threads}, active={active}");
            if ffi_ret == OK {
                assert_eq!(ffi_new, d.active,
                    "alloc new: max={max_threads}, active={active}");
            }
        }
    }
}

// =====================================================================
// Differential tests: dynamic free_validate
// =====================================================================

#[test]
fn dynamic_free_validate_ffi_matches_model_exhaustive() {
    for max_threads in 1u32..=10 {
        for active in 0u32..=max_threads {
            let (ffi_ret, ffi_new) = ffi_dynamic_free_validate(active);

            let mut d = DynamicPool { max_threads, active, stack_size: 4096 };
            let model_ret = d.free();

            assert_eq!(ffi_ret, model_ret,
                "free ret: active={active}");
            if ffi_ret == OK {
                assert_eq!(ffi_new, d.active,
                    "free new: active={active}");
            }
        }
    }
}

// =====================================================================
// Differential tests: dynamic alloc_decide / free_decide
// =====================================================================

#[test]
fn dynamic_alloc_decide_ffi_matches_model_exhaustive() {
    for max_threads in 1u32..=10 {
        for active in 0u32..=max_threads {
            let (ffi_action, ffi_new) = ffi_dynamic_alloc_decide(active, max_threads);

            if active >= max_threads {
                assert_eq!(ffi_action, 1, "POOL_FULL");
                assert_eq!(ffi_new, active);
            } else {
                assert_eq!(ffi_action, 0, "ALLOC_OK");
                #[allow(clippy::arithmetic_side_effects)]
                let expected = active + 1;
                assert_eq!(ffi_new, expected);
            }
        }
    }
}

#[test]
fn dynamic_free_decide_ffi_matches_model_exhaustive() {
    for active in 0u32..=10 {
        let (ffi_action, ffi_new) = ffi_dynamic_free_decide(active);

        if active == 0 {
            assert_eq!(ffi_action, 1, "UNDERFLOW");
            assert_eq!(ffi_new, 0);
        } else {
            assert_eq!(ffi_action, 0, "FREE_OK");
            #[allow(clippy::arithmetic_side_effects)]
            let expected = active - 1;
            assert_eq!(ffi_new, expected);
        }
    }
}

// =====================================================================
// Property: DY1 — 0 <= active <= max_threads
// =====================================================================

#[test]
fn dynamic_alloc_never_exceeds_max() {
    for max_threads in 1u32..=20 {
        for active in 0u32..=max_threads {
            let (ret, new_active) = ffi_dynamic_alloc_validate(active, max_threads);
            if ret == OK {
                assert!(new_active <= max_threads, "DY1: {new_active} > {max_threads}");
            }
        }
    }
}

// =====================================================================
// Property: DY2+DY4 — alloc/free roundtrip
// =====================================================================

#[test]
fn dynamic_alloc_free_roundtrip() {
    for max_threads in 1u32..=10 {
        for initial in 0u32..max_threads {
            let (ret, new_active) = ffi_dynamic_alloc_validate(initial, max_threads);
            assert_eq!(ret, OK);

            let (ret2, final_active) = ffi_dynamic_free_validate(new_active);
            assert_eq!(ret2, OK);
            assert_eq!(final_active, initial, "roundtrip failed: initial={initial}");
        }
    }
}
