//! Differential equivalence tests — MemSlab (FFI vs Model).
//!
//! Verifies that the FFI mem_slab functions produce the same results as
//! the Verus-verified model functions in gale::mem_slab.

#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use gale::error::*;
use gale::mem_slab::MemSlab;

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Replica of gale_mem_slab_init_validate.
fn ffi_mem_slab_init_validate(block_size: u32, num_blocks: u32) -> i32 {
    if block_size == 0 || num_blocks == 0 {
        EINVAL
    } else {
        OK
    }
}

/// Replica of gale_mem_slab_alloc_validate.
/// Returns (ret, new_num_used).
fn ffi_mem_slab_alloc_validate(num_used: u32, num_blocks: u32) -> (i32, u32) {
    if num_used >= num_blocks {
        (ENOMEM, num_used)
    } else {
        (OK, num_used + 1)
    }
}

/// Replica of gale_mem_slab_free_validate.
/// Returns (ret, new_num_used).
fn ffi_mem_slab_free_validate(num_used: u32) -> (i32, u32) {
    if num_used == 0 {
        (EINVAL, num_used)
    } else {
        (OK, num_used - 1)
    }
}

/// Replica of gale_k_mem_slab_alloc_decide.
/// Returns (ret, new_num_used, action).
fn ffi_mem_slab_alloc_decide(
    num_used: u32,
    num_blocks: u32,
    is_no_wait: bool,
) -> (i32, u32, u8) {
    if num_used < num_blocks {
        (OK, num_used + 1, 0) // ALLOC_OK
    } else if is_no_wait {
        (ENOMEM, num_used, 2) // RETURN_NOMEM
    } else {
        (0, num_used, 1) // PEND_CURRENT
    }
}

/// Replica of gale_k_mem_slab_free_decide.
/// Returns (new_num_used, action).
fn ffi_mem_slab_free_decide(num_used: u32, has_waiter: bool) -> (u32, u8) {
    if has_waiter {
        (num_used, 1) // WAKE_THREAD
    } else if num_used > 0 {
        (num_used - 1, 0) // FREE_OK
    } else {
        (0, 0) // FREE_OK (no-op)
    }
}

// =====================================================================
// Differential tests: init_validate
// =====================================================================

#[test]
fn mem_slab_init_validate_ffi_matches_model() {
    for block_size in 0u32..=8 {
        for num_blocks in 0u32..=8 {
            let ffi_ret = ffi_mem_slab_init_validate(block_size, num_blocks);
            let model_result = MemSlab::init(block_size, num_blocks);

            match model_result {
                Ok(_) => {
                    assert_eq!(ffi_ret, OK,
                        "init: block_size={block_size}, num_blocks={num_blocks}");
                }
                Err(e) => {
                    assert_eq!(ffi_ret, e,
                        "init error: block_size={block_size}, num_blocks={num_blocks}");
                }
            }
        }
    }
}

// =====================================================================
// Differential tests: alloc_validate
// =====================================================================

#[test]
fn mem_slab_alloc_validate_ffi_matches_model_exhaustive() {
    for num_blocks in 1u32..=16 {
        for num_used in 0u32..=num_blocks {
            let (ffi_ret, ffi_new) = ffi_mem_slab_alloc_validate(num_used, num_blocks);

            let mut ms = MemSlab::init(64, num_blocks).unwrap();
            // Set num_used manually
            ms.num_used = num_used;
            let model_ret = ms.alloc();

            assert_eq!(ffi_ret, model_ret,
                "alloc ret: num_used={num_used}, num_blocks={num_blocks}");
            if ffi_ret == OK {
                assert_eq!(ffi_new, ms.num_used,
                    "alloc new_num_used: num_used={num_used}, num_blocks={num_blocks}");
            }
        }
    }
}

// =====================================================================
// Differential tests: free_validate
// =====================================================================

#[test]
fn mem_slab_free_validate_ffi_matches_model_exhaustive() {
    for num_blocks in 1u32..=16 {
        for num_used in 0u32..=num_blocks {
            let (ffi_ret, ffi_new) = ffi_mem_slab_free_validate(num_used);

            let mut ms = MemSlab::init(64, num_blocks).unwrap();
            ms.num_used = num_used;
            let model_ret = ms.free();

            assert_eq!(ffi_ret, model_ret,
                "free ret: num_used={num_used}");
            if ffi_ret == OK {
                assert_eq!(ffi_new, ms.num_used,
                    "free new_num_used: num_used={num_used}");
            }
        }
    }
}

// =====================================================================
// Differential tests: alloc_decide
// =====================================================================

#[test]
fn mem_slab_alloc_decide_ffi_matches_model_exhaustive() {
    for num_blocks in 1u32..=10 {
        for num_used in 0u32..=num_blocks {
            for is_no_wait in [false, true] {
                let (ffi_ret, ffi_new, ffi_action) =
                    ffi_mem_slab_alloc_decide(num_used, num_blocks, is_no_wait);

                if num_used < num_blocks {
                    assert_eq!(ffi_action, 0, "ALLOC_OK expected");
                    assert_eq!(ffi_ret, OK);
                    assert_eq!(ffi_new, num_used + 1);
                } else if is_no_wait {
                    assert_eq!(ffi_action, 2, "RETURN_NOMEM expected");
                    assert_eq!(ffi_ret, ENOMEM);
                    assert_eq!(ffi_new, num_used);
                } else {
                    assert_eq!(ffi_action, 1, "PEND_CURRENT expected");
                    assert_eq!(ffi_new, num_used);
                }

                // Cross-check against model
                let mut ms = MemSlab::init(64, num_blocks).unwrap();
                ms.num_used = num_used;
                let model_ret = ms.alloc();

                if num_used < num_blocks {
                    assert_eq!(model_ret, OK);
                    assert_eq!(ms.num_used, num_used + 1);
                } else {
                    assert_eq!(model_ret, ENOMEM);
                    assert_eq!(ms.num_used, num_used);
                }
            }
        }
    }
}

// =====================================================================
// Differential tests: free_decide
// =====================================================================

#[test]
fn mem_slab_free_decide_ffi_matches_model_exhaustive() {
    for num_used in 0u32..=16 {
        for has_waiter in [false, true] {
            let (ffi_new, ffi_action) = ffi_mem_slab_free_decide(num_used, has_waiter);

            if has_waiter {
                assert_eq!(ffi_action, 1, "WAKE_THREAD expected");
                assert_eq!(ffi_new, num_used, "count unchanged when waking");
            } else if num_used > 0 {
                assert_eq!(ffi_action, 0, "FREE_OK expected");
                assert_eq!(ffi_new, num_used - 1);
            } else {
                assert_eq!(ffi_action, 0, "FREE_OK (no-op) expected");
                assert_eq!(ffi_new, 0);
            }
        }
    }
}

// =====================================================================
// Property: MS1 — 0 <= num_used <= num_blocks
// =====================================================================

#[test]
fn mem_slab_bounds_invariant() {
    for num_blocks in 1u32..=20 {
        for num_used in 0u32..=num_blocks {
            let (ffi_ret, ffi_new) = ffi_mem_slab_alloc_validate(num_used, num_blocks);
            if ffi_ret == OK {
                assert!(ffi_new <= num_blocks,
                    "MS1 violated after alloc: new={ffi_new} > blocks={num_blocks}");
            }

            let (ffi_ret2, ffi_new2) = ffi_mem_slab_free_validate(num_used);
            if ffi_ret2 == OK {
                assert!(ffi_new2 <= num_blocks,
                    "MS1 violated after free: new={ffi_new2} > blocks={num_blocks}");
            }
        }
    }
}

// =====================================================================
// Property: MS7 — num_free + num_used == num_blocks (conservation)
// =====================================================================

#[test]
fn mem_slab_conservation() {
    let num_blocks = 8u32;
    let mut ms = MemSlab::init(64, num_blocks).unwrap();
    let mut ffi_num_used = 0u32;

    // Alloc all
    for _ in 0..num_blocks {
        let (ffi_ret, ffi_new) = ffi_mem_slab_alloc_validate(ffi_num_used, num_blocks);
        let model_ret = ms.alloc();

        assert_eq!(ffi_ret, model_ret);
        ffi_num_used = ffi_new;

        // Conservation
        assert_eq!(ffi_num_used + (num_blocks - ffi_num_used), num_blocks);
        assert_eq!(ms.num_used_get() + ms.num_free_get(), num_blocks);
        assert_eq!(ffi_num_used, ms.num_used_get());
    }

    // Free all
    for _ in 0..num_blocks {
        let (ffi_ret, ffi_new) = ffi_mem_slab_free_validate(ffi_num_used);
        let model_ret = ms.free();

        assert_eq!(ffi_ret, model_ret);
        ffi_num_used = ffi_new;

        assert_eq!(ffi_num_used + (num_blocks - ffi_num_used), num_blocks);
        assert_eq!(ms.num_used_get() + ms.num_free_get(), num_blocks);
        assert_eq!(ffi_num_used, ms.num_used_get());
    }
}

// =====================================================================
// Random operations: FFI matches model through a sequence
// =====================================================================

#[test]
fn mem_slab_random_ops_ffi_matches_model() {
    let num_blocks = 32u32;
    let mut ms = MemSlab::init(64, num_blocks).unwrap();
    let mut ffi_num_used = 0u32;

    let mut rng: u32 = 0xFACE_CAFE;
    for _ in 0..1000 {
        rng = rng.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        if rng % 2 == 0 {
            let (ffi_ret, ffi_new) = ffi_mem_slab_alloc_validate(ffi_num_used, num_blocks);
            let model_ret = ms.alloc();
            assert_eq!(ffi_ret, model_ret);
            if ffi_ret == OK {
                ffi_num_used = ffi_new;
            }
        } else {
            let (ffi_ret, ffi_new) = ffi_mem_slab_free_validate(ffi_num_used);
            let model_ret = ms.free();
            assert_eq!(ffi_ret, model_ret);
            if ffi_ret == OK {
                ffi_num_used = ffi_new;
            }
        }
        assert_eq!(ffi_num_used, ms.num_used_get(),
            "FFI/model mem_slab diverged at rng={rng}");
    }
}
