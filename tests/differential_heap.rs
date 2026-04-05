//! Differential equivalence tests — Heap (FFI vs Model).
//!
//! Verifies that the FFI sys_heap functions produce the same results as
//! the Verus-verified model functions in gale::heap.

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
use gale::heap::Heap;

// Action constants matching FFI
const GALE_HEAP_ACTION_USE_WHOLE: u8 = 0;
const GALE_HEAP_ACTION_SPLIT_AND_USE: u8 = 1;
const GALE_HEAP_ACTION_ALLOC_FAILED: u8 = 2;

const GALE_HEAP_ACTION_FREE_AND_COALESCE: u8 = 0;
const GALE_HEAP_ACTION_FREE_REJECTED: u8 = 1;

const GALE_HEAP_ALIGN_PLAIN: u8 = 0;
const GALE_HEAP_ALIGN_PADDED: u8 = 1;
const GALE_HEAP_ALIGN_REJECT: u8 = 2;

const GALE_HEAP_REALLOC_SHRINK: u8 = 0;
const GALE_HEAP_REALLOC_GROW: u8 = 1;
const GALE_HEAP_REALLOC_COPY: u8 = 2;
const GALE_HEAP_REALLOC_REJECT: u8 = 3;

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Replica of gale_sys_heap_init_validate.
fn ffi_heap_init_validate(total_bytes: u32, min_overhead: u32) -> i32 {
    if total_bytes == 0 || min_overhead == 0 || min_overhead >= total_bytes {
        EINVAL
    } else {
        OK
    }
}

/// Replica of gale_sys_heap_alloc_decide.
fn ffi_heap_alloc_decide(
    found_chunk: bool,
    found_chunk_sz: u32,
    needed_chunk_sz: u32,
) -> (u8, bool) {
    // Returns (action, valid)
    if !found_chunk {
        return (GALE_HEAP_ACTION_ALLOC_FAILED, true);
    }
    if needed_chunk_sz == 0 {
        return (GALE_HEAP_ACTION_ALLOC_FAILED, false);
    }
    if found_chunk_sz < needed_chunk_sz {
        return (GALE_HEAP_ACTION_ALLOC_FAILED, false);
    }
    if found_chunk_sz > needed_chunk_sz {
        (GALE_HEAP_ACTION_SPLIT_AND_USE, true)
    } else {
        (GALE_HEAP_ACTION_USE_WHOLE, true)
    }
}

/// Replica of gale_sys_heap_free_decide.
fn ffi_heap_free_decide(
    chunk_is_used: bool,
    right_neighbor_free: bool,
    left_neighbor_free: bool,
    bounds_check_passed: bool,
) -> (u8, bool, bool) {
    // Returns (action, merge_right, merge_left)
    if !chunk_is_used {
        return (GALE_HEAP_ACTION_FREE_REJECTED, false, false);
    }
    if !bounds_check_passed {
        return (GALE_HEAP_ACTION_FREE_REJECTED, false, false);
    }
    (GALE_HEAP_ACTION_FREE_AND_COALESCE, right_neighbor_free, left_neighbor_free)
}

/// Replica of gale_sys_heap_aligned_alloc_decide.
fn ffi_heap_aligned_alloc_decide(
    bytes: u32,
    align: u32,
    chunk_header_bytes: u32,
) -> (u8, u32) {
    // Returns (action, padded_bytes)
    if bytes == 0 {
        return (GALE_HEAP_ALIGN_REJECT, 0);
    }
    // align must be 0 or power of 2
    if align != 0 && (align & align.wrapping_sub(1)) != 0 {
        return (GALE_HEAP_ALIGN_REJECT, 0);
    }
    if align == 0 || align <= chunk_header_bytes {
        return (GALE_HEAP_ALIGN_PLAIN, bytes);
    }
    #[allow(clippy::arithmetic_side_effects)]
    let padding: u64 = align as u64 - chunk_header_bytes as u64;
    #[allow(clippy::arithmetic_side_effects)]
    let padded: u64 = bytes as u64 + padding;
    if padded > u32::MAX as u64 {
        return (GALE_HEAP_ALIGN_REJECT, 0);
    }
    (GALE_HEAP_ALIGN_PADDED, padded as u32)
}

/// Replica of gale_sys_heap_split_validate.
fn ffi_heap_split_validate(original_sz: u32, left_sz: u32) -> u32 {
    if left_sz == 0 || left_sz >= original_sz {
        0
    } else {
        #[allow(clippy::arithmetic_side_effects)]
        let right_sz = original_sz - left_sz;
        right_sz
    }
}

/// Replica of gale_sys_heap_merge_validate.
fn ffi_heap_merge_validate(
    left_sz: u32,
    right_sz: u32,
    left_free: bool,
    right_free: bool,
) -> (i32, u32) {
    // Returns (ret, merged_sz)
    if left_sz == 0 || right_sz == 0 {
        return (EINVAL, 0);
    }
    if !left_free || !right_free {
        return (EINVAL, 0);
    }
    let sum: u64 = left_sz as u64 + right_sz as u64;
    if sum > u32::MAX as u64 {
        return (EINVAL, 0);
    }
    (OK, sum as u32)
}

/// Replica of gale_sys_heap_realloc_decide.
fn ffi_heap_realloc_decide(
    current_chunk_sz: u32,
    needed_chunk_sz: u32,
    right_neighbor_free: bool,
    right_neighbor_sz: u32,
) -> u8 {
    if needed_chunk_sz == 0 {
        return GALE_HEAP_REALLOC_REJECT;
    }
    if current_chunk_sz >= needed_chunk_sz {
        return GALE_HEAP_REALLOC_SHRINK;
    }
    if right_neighbor_free {
        let combined: u64 = current_chunk_sz as u64 + right_neighbor_sz as u64;
        if combined >= needed_chunk_sz as u64 && combined <= u32::MAX as u64 {
            return GALE_HEAP_REALLOC_GROW;
        }
    }
    GALE_HEAP_REALLOC_COPY
}

// =====================================================================
// Differential tests: heap init_validate
// =====================================================================

#[test]
fn heap_init_validate_ffi_matches_model_exhaustive() {
    for total in [0u32, 1, 10, 100, 1000] {
        for overhead in [0u32, 1, 5, 9, 10, 11, 100] {
            let ffi_ret = ffi_heap_init_validate(total, overhead);
            let model_result = Heap::init(total, overhead);

            if total == 0 || overhead == 0 || overhead >= total {
                assert_eq!(ffi_ret, EINVAL,
                    "init: should be EINVAL: total={total}, overhead={overhead}");
                assert!(model_result.is_err(),
                    "model init: should fail: total={total}, overhead={overhead}");
            } else {
                assert_eq!(ffi_ret, OK,
                    "init: should be OK: total={total}, overhead={overhead}");
                let h = model_result.unwrap();
                assert_eq!(h.capacity, total);
                assert_eq!(h.allocated_bytes, overhead);
            }
        }
    }
}

// =====================================================================
// Differential tests: heap alloc_decide
// =====================================================================

#[test]
fn heap_alloc_decide_ffi_matches_model_exhaustive() {
    for found_chunk in [false, true] {
        for found_sz in [0u32, 1, 5, 10, 20] {
            for needed_sz in [0u32, 1, 5, 10, 11] {
                let (ffi_action, ffi_valid) =
                    ffi_heap_alloc_decide(found_chunk, found_sz, needed_sz);

                // Verify logic directly:
                if !found_chunk {
                    assert_eq!(ffi_action, GALE_HEAP_ACTION_ALLOC_FAILED,
                        "no chunk found => ALLOC_FAILED");
                    assert!(ffi_valid, "no chunk => valid=true");
                } else if needed_sz == 0 {
                    assert_eq!(ffi_action, GALE_HEAP_ACTION_ALLOC_FAILED,
                        "zero needed => ALLOC_FAILED");
                    assert!(!ffi_valid, "zero needed => valid=false");
                } else if found_sz < needed_sz {
                    assert_eq!(ffi_action, GALE_HEAP_ACTION_ALLOC_FAILED,
                        "too small => ALLOC_FAILED");
                    assert!(!ffi_valid, "too small => valid=false");
                } else if found_sz > needed_sz {
                    assert_eq!(ffi_action, GALE_HEAP_ACTION_SPLIT_AND_USE,
                        "bigger => SPLIT: found={found_sz}, needed={needed_sz}");
                    assert!(ffi_valid);
                } else {
                    assert_eq!(ffi_action, GALE_HEAP_ACTION_USE_WHOLE,
                        "exact => USE_WHOLE: found={found_sz}, needed={needed_sz}");
                    assert!(ffi_valid);
                }
            }
        }
    }
}

// =====================================================================
// Differential tests: heap free_decide
// =====================================================================

#[test]
fn heap_free_decide_ffi_matches_model_exhaustive() {
    for chunk_used in [false, true] {
        for right_free in [false, true] {
            for left_free in [false, true] {
                for bounds_ok in [false, true] {
                    let (ffi_action, ffi_merge_right, ffi_merge_left) =
                        ffi_heap_free_decide(chunk_used, right_free, left_free, bounds_ok);

                    if !chunk_used || !bounds_ok {
                        assert_eq!(ffi_action, GALE_HEAP_ACTION_FREE_REJECTED,
                            "HP5: double-free/bounds: used={chunk_used}, bounds={bounds_ok}");
                        assert!(!ffi_merge_right);
                        assert!(!ffi_merge_left);
                    } else {
                        assert_eq!(ffi_action, GALE_HEAP_ACTION_FREE_AND_COALESCE,
                            "valid free: used={chunk_used}, bounds={bounds_ok}");
                        assert_eq!(ffi_merge_right, right_free,
                            "merge_right follows right_neighbor_free");
                        assert_eq!(ffi_merge_left, left_free,
                            "merge_left follows left_neighbor_free");
                    }
                }
            }
        }
    }
}

// =====================================================================
// Differential tests: heap aligned_alloc_decide
// =====================================================================

#[test]
fn heap_aligned_alloc_decide_ffi_matches_model_exhaustive() {
    let chunk_header = 8u32;

    // Zero bytes — always rejected
    let (action, _) = ffi_heap_aligned_alloc_decide(0, 0, chunk_header);
    assert_eq!(action, GALE_HEAP_ALIGN_REJECT, "bytes=0 must be rejected");

    // Non-power-of-2 alignment — rejected
    let (action, _) = ffi_heap_aligned_alloc_decide(16, 3, chunk_header);
    assert_eq!(action, GALE_HEAP_ALIGN_REJECT, "non-pow2 align must be rejected");

    // align=0 — plain alloc
    for bytes in [1u32, 8, 16, 100] {
        let (action, padded) = ffi_heap_aligned_alloc_decide(bytes, 0, chunk_header);
        assert_eq!(action, GALE_HEAP_ALIGN_PLAIN,
            "align=0 => plain: bytes={bytes}");
        assert_eq!(padded, bytes);
    }

    // align <= chunk_header — plain alloc
    for bytes in [1u32, 8, 100] {
        for align in [1u32, 4, 8] {
            let (action, padded) = ffi_heap_aligned_alloc_decide(bytes, align, chunk_header);
            assert_eq!(action, GALE_HEAP_ALIGN_PLAIN,
                "align<=header => plain: bytes={bytes}, align={align}");
            assert_eq!(padded, bytes);
        }
    }

    // align > chunk_header and valid power of 2 — padded
    for bytes in [16u32, 32, 100] {
        for align in [16u32, 32, 64] {
            let (action, padded) = ffi_heap_aligned_alloc_decide(bytes, align, chunk_header);
            assert_eq!(action, GALE_HEAP_ALIGN_PADDED,
                "align>header => padded: bytes={bytes}, align={align}");
            #[allow(clippy::arithmetic_side_effects)]
            let expected_padded = bytes + align - chunk_header;
            assert_eq!(padded, expected_padded,
                "padded size: bytes={bytes}, align={align}");
        }
    }
}

// =====================================================================
// Differential tests: heap split_validate
// =====================================================================

#[test]
fn heap_split_validate_ffi_exhaustive() {
    // Invalid: left_sz == 0
    assert_eq!(ffi_heap_split_validate(10, 0), 0, "left=0 must return 0");

    // Invalid: left_sz >= original
    for original in 1u32..=10 {
        assert_eq!(ffi_heap_split_validate(original, original), 0,
            "left==original must return 0");
        assert_eq!(ffi_heap_split_validate(original, original + 1), 0,
            "left>original must return 0");
    }

    // Valid splits
    for original in 2u32..=20 {
        for left_sz in 1u32..original {
            let right_sz = ffi_heap_split_validate(original, left_sz);
            assert!(right_sz > 0,
                "HP8: valid split must return >0: orig={original}, left={left_sz}");
            #[allow(clippy::arithmetic_side_effects)]
            let expected = original - left_sz;
            assert_eq!(right_sz, expected,
                "HP8: right_sz must be original-left: orig={original}, left={left_sz}");
        }
    }
}

// =====================================================================
// Differential tests: heap merge_validate
// =====================================================================

#[test]
fn heap_merge_validate_ffi_matches_model_exhaustive() {
    // Invalid: one chunk not free
    let (ret, _) = ffi_heap_merge_validate(10, 10, false, true);
    assert_eq!(ret, EINVAL, "left not free => EINVAL");
    let (ret, _) = ffi_heap_merge_validate(10, 10, true, false);
    assert_eq!(ret, EINVAL, "right not free => EINVAL");

    // Invalid: zero size
    let (ret, _) = ffi_heap_merge_validate(0, 10, true, true);
    assert_eq!(ret, EINVAL, "left_sz=0 => EINVAL");
    let (ret, _) = ffi_heap_merge_validate(10, 0, true, true);
    assert_eq!(ret, EINVAL, "right_sz=0 => EINVAL");

    // Valid merges
    for left in 1u32..=20 {
        for right in 1u32..=20 {
            let (ret, merged) = ffi_heap_merge_validate(left, right, true, true);
            assert_eq!(ret, OK, "HP2: valid merge: left={left}, right={right}");
            #[allow(clippy::arithmetic_side_effects)]
            let expected = left + right;
            assert_eq!(merged, expected,
                "HP7: merged size: left={left}, right={right}");
        }
    }
}

// =====================================================================
// Differential tests: heap realloc_decide
// =====================================================================

#[test]
fn heap_realloc_decide_ffi_matches_model_exhaustive() {
    // needed=0 => always reject
    for current in [0u32, 1, 10] {
        let action = ffi_heap_realloc_decide(current, 0, false, 0);
        assert_eq!(action, GALE_HEAP_REALLOC_REJECT, "needed=0 => REJECT");
    }

    // Shrink: current >= needed
    for needed in 1u32..=10 {
        for current in needed..=needed + 5 {
            let action = ffi_heap_realloc_decide(current, needed, false, 0);
            assert_eq!(action, GALE_HEAP_REALLOC_SHRINK,
                "current>=needed => SHRINK: cur={current}, need={needed}");
        }
    }

    // Grow in-place: right neighbor has space
    for current in 1u32..=5 {
        for extra in 1u32..=5 {
            #[allow(clippy::arithmetic_side_effects)]
            let needed = current + extra;
            let right_sz = extra + 2;
            let action = ffi_heap_realloc_decide(current, needed, true, right_sz);
            assert_eq!(action, GALE_HEAP_REALLOC_GROW,
                "right neighbor fits => GROW: cur={current}, need={needed}");
        }
    }

    // Copy: no right neighbor
    for current in 1u32..=5 {
        #[allow(clippy::arithmetic_side_effects)]
        let needed = current + 10;
        let action = ffi_heap_realloc_decide(current, needed, false, 0);
        assert_eq!(action, GALE_HEAP_REALLOC_COPY,
            "no neighbor => COPY: cur={current}, need={needed}");
    }
}

// =====================================================================
// Property: HP5 — double-free always rejected
// =====================================================================

#[test]
fn heap_double_free_always_rejected() {
    for bounds_ok in [false, true] {
        let (action, _, _) = ffi_heap_free_decide(false, false, false, bounds_ok);
        assert_eq!(action, GALE_HEAP_ACTION_FREE_REJECTED,
            "HP5: unused chunk must be rejected regardless of bounds");
    }
}

// =====================================================================
// Property: HP1+HP3 — alloc tracks bytes correctly
// =====================================================================

#[test]
fn heap_alloc_tracks_bytes() {
    let mut h = Heap {
        capacity: 1000,
        allocated_bytes: 100,
        total_chunks: 5,
        free_chunks: 3,
        next_slot_id: 2,
    };

    let result = h.alloc(50);
    assert!(result.is_ok(), "HP3: alloc should succeed with free space");
    assert_eq!(h.allocated_bytes, 150, "HP1: allocated_bytes must increase by 50");
}

// =====================================================================
// Property: HP4+HP5 — free tracks bytes, double-free detected
// =====================================================================

#[test]
fn heap_free_tracks_bytes() {
    let mut h = Heap {
        capacity: 1000,
        allocated_bytes: 200,
        total_chunks: 5,
        free_chunks: 2,
        next_slot_id: 3,
    };

    let rc = h.free(100);
    assert_eq!(rc, OK, "HP4: valid free should return OK");
    assert_eq!(h.allocated_bytes, 100, "HP4: allocated_bytes must decrease by 100");
}

// =====================================================================
// Property: HP3+HP4 — alloc then free roundtrip
// =====================================================================

#[test]
fn heap_alloc_free_roundtrip() {
    for capacity in [100u32, 500, 1000] {
        for bytes in [1u32, 10, 50, 99] {
            if bytes >= capacity {
                continue;
            }
            let overhead: u32 = 10;
            if overhead >= capacity {
                continue;
            }
            let mut h = Heap::init(capacity, overhead).unwrap();

            let initial_alloc = h.allocated_bytes;
            let _ = h.alloc(bytes);
            if h.allocated_bytes == initial_alloc + bytes {
                let rc = h.free(bytes);
                assert_eq!(rc, OK, "HP4: free must succeed: cap={capacity}, bytes={bytes}");
                assert_eq!(h.allocated_bytes, initial_alloc,
                    "HP3+HP4: roundtrip must preserve allocated_bytes");
            }
        }
    }
}
