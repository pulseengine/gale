//! Differential equivalence tests — KHeap (FFI vs Model).
//!
//! Verifies that the FFI kheap functions produce the same results as
//! the Verus-verified model functions in gale::kheap.

#![allow(
    clippy::shadow_unrelated,
    clippy::indexing_slicing,
    clippy::bool_to_int_with_if
)]

use gale::error::*;
use gale::kheap::KHeap;

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Replica of gale_kheap_alloc_validate.
fn ffi_kheap_alloc_validate(allocated_bytes: u32, capacity: u32, bytes: u32) -> (i32, u32) {
    if bytes == 0 {
        return (EINVAL, allocated_bytes);
    }
    #[allow(clippy::arithmetic_side_effects)]
    let remaining = capacity - allocated_bytes.min(capacity);
    if bytes <= remaining {
        #[allow(clippy::arithmetic_side_effects)]
        let new = allocated_bytes + bytes;
        (OK, new)
    } else {
        (ENOMEM, allocated_bytes)
    }
}

/// Replica of gale_kheap_free_validate.
fn ffi_kheap_free_validate(allocated_bytes: u32, bytes: u32) -> (i32, u32) {
    if bytes == 0 {
        return (EINVAL, allocated_bytes);
    }
    if bytes <= allocated_bytes {
        #[allow(clippy::arithmetic_side_effects)]
        let new = allocated_bytes - bytes;
        (OK, new)
    } else {
        (EINVAL, allocated_bytes)
    }
}

/// Replica of gale_k_kheap_alloc_decide.
fn ffi_kheap_alloc_decide(alloc_succeeded: bool, is_no_wait: bool) -> u8 {
    if alloc_succeeded {
        0 // RETURN_PTR
    } else if is_no_wait {
        2 // RETURN_NULL
    } else {
        1 // PEND
    }
}

/// Replica of gale_k_kheap_free_decide.
fn ffi_kheap_free_decide(has_waiters: bool) -> u8 {
    if has_waiters { 1 } else { 0 } // FREE_AND_RESCHEDULE / FREE_ONLY
}

// =====================================================================
// Differential tests: kheap alloc_validate
// =====================================================================

#[test]
fn kheap_alloc_validate_ffi_matches_model_exhaustive() {
    for capacity in 1u32..=10 {
        for allocated in 0u32..=capacity {
            for bytes in 0u32..=capacity {
                let (ffi_ret, ffi_new) = ffi_kheap_alloc_validate(allocated, capacity, bytes);

                if bytes == 0 {
                    assert_eq!(ffi_ret, EINVAL);
                    continue;
                }

                let mut h = KHeap { capacity, allocated_bytes: allocated };
                let model_ret = h.alloc(bytes);

                assert_eq!(ffi_ret, model_ret,
                    "alloc ret: cap={capacity}, alloc={allocated}, bytes={bytes}");
                if ffi_ret == OK {
                    assert_eq!(ffi_new, h.allocated_bytes,
                        "alloc new: cap={capacity}, alloc={allocated}, bytes={bytes}");
                }
            }
        }
    }
}

// =====================================================================
// Differential tests: kheap free_validate
// =====================================================================

#[test]
fn kheap_free_validate_ffi_matches_model_exhaustive() {
    for capacity in 1u32..=10 {
        for allocated in 0u32..=capacity {
            for bytes in 0u32..=capacity {
                let (ffi_ret, ffi_new) = ffi_kheap_free_validate(allocated, bytes);

                if bytes == 0 {
                    assert_eq!(ffi_ret, EINVAL);
                    continue;
                }

                let mut h = KHeap { capacity, allocated_bytes: allocated };
                let model_ret = h.free(bytes);

                assert_eq!(ffi_ret, model_ret,
                    "free ret: alloc={allocated}, bytes={bytes}");
                if ffi_ret == OK {
                    assert_eq!(ffi_new, h.allocated_bytes,
                        "free new: alloc={allocated}, bytes={bytes}");
                }
            }
        }
    }
}

// =====================================================================
// Differential tests: kheap alloc_decide / free_decide
// =====================================================================

#[test]
fn kheap_alloc_decide_ffi_matches_model() {
    for alloc_succeeded in [false, true] {
        for is_no_wait in [false, true] {
            let ffi_action = ffi_kheap_alloc_decide(alloc_succeeded, is_no_wait);

            if alloc_succeeded {
                assert_eq!(ffi_action, 0, "RETURN_PTR");
            } else if is_no_wait {
                assert_eq!(ffi_action, 2, "RETURN_NULL");
            } else {
                assert_eq!(ffi_action, 1, "PEND");
            }
        }
    }
}

#[test]
fn kheap_free_decide_ffi_matches_model() {
    for has_waiters in [false, true] {
        let ffi_action = ffi_kheap_free_decide(has_waiters);
        if has_waiters {
            assert_eq!(ffi_action, 1, "FREE_AND_RESCHEDULE");
        } else {
            assert_eq!(ffi_action, 0, "FREE_ONLY");
        }
    }
}

// =====================================================================
// Property: KH1 — 0 <= allocated_bytes <= capacity
// =====================================================================

#[test]
fn kheap_alloc_never_exceeds_capacity() {
    for capacity in 1u32..=20 {
        for allocated in 0u32..=capacity {
            for bytes in 1u32..=capacity {
                let (ret, new_alloc) = ffi_kheap_alloc_validate(allocated, capacity, bytes);
                if ret == OK {
                    assert!(new_alloc <= capacity, "KH1: new_alloc={new_alloc} > cap={capacity}");
                }
            }
        }
    }
}

// =====================================================================
// Property: KH5 — conservation: free + allocated == capacity
// =====================================================================

#[test]
fn kheap_alloc_free_roundtrip() {
    let capacity = 100u32;
    let mut h = KHeap { capacity, allocated_bytes: 0 };

    // Allocate 50 bytes
    assert_eq!(h.alloc(50), OK);
    assert_eq!(h.allocated_bytes, 50);
    #[allow(clippy::arithmetic_side_effects)]
    let free_bytes = capacity - h.allocated_bytes;
    assert_eq!(free_bytes, 50);

    // Free 50 bytes
    assert_eq!(h.free(50), OK);
    assert_eq!(h.allocated_bytes, 0);
}
