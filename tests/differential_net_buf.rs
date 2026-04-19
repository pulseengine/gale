//! Differential equivalence tests — NetBuf (FFI vs Model).
//!
//! Verifies that the FFI net_buf decision functions produce the same results
//! as the Verus-verified model functions in gale::net_buf.

#![allow(
    clippy::shadow_unrelated,
    clippy::indexing_slicing,
    clippy::bool_to_int_with_if,
    clippy::unwrap_used,
    clippy::fn_params_excessive_bools,
    clippy::absurd_extreme_comparisons,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::checked_conversions,
    clippy::wildcard_enum_match_arm,
    clippy::implicit_saturating_sub,
    clippy::branches_sharing_code,
    clippy::panic
)]

use gale::error::*;
use gale::net_buf::{alloc_decide, free_decide, ref_decide, unref_decide, add_decide,
                    remove_decide, push_decide, pull_decide};

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Replica of gale_net_buf_alloc_decide.
fn ffi_net_buf_alloc_decide(allocated: u16, capacity: u16) -> (u16, i32) {
    if allocated < capacity {
        (allocated + 1, OK)
    } else {
        (allocated, ENOMEM)
    }
}

/// Replica of gale_net_buf_free_decide.
fn ffi_net_buf_free_decide(allocated: u16) -> i32 {
    if allocated > 0 {
        OK
    } else {
        EINVAL
    }
}

/// Replica of gale_net_buf_ref_decide.
fn ffi_net_buf_ref_decide(ref_count: u8) -> (u8, i32) {
    if ref_count < u8::MAX {
        (ref_count + 1, OK)
    } else {
        (ref_count, EOVERFLOW)
    }
}

/// Replica of gale_net_buf_unref_decide.
fn ffi_net_buf_unref_decide(ref_count: u8) -> (u8, u8, i32) {
    // returns (new_ref_count, should_free, rc)
    if ref_count == 0 {
        (ref_count, 0, EINVAL)
    } else {
        let new_ref = ref_count - 1;
        let should_free: u8 = if new_ref == 0 { 1 } else { 0 };
        (new_ref, should_free, OK)
    }
}

/// Replica of gale_net_buf_add_decide.
fn ffi_net_buf_add_decide(head_offset: u16, len: u16, size: u16, bytes: u16) -> (u16, u16, i32) {
    let tailroom: u16 = size - head_offset - len;
    if bytes > tailroom {
        (head_offset, len, ENOMEM)
    } else {
        (head_offset, len + bytes, OK)
    }
}

/// Replica of gale_net_buf_remove_decide.
fn ffi_net_buf_remove_decide(head_offset: u16, len: u16, bytes: u16) -> (u16, u16, i32) {
    if bytes > len {
        (head_offset, len, EINVAL)
    } else {
        (head_offset, len - bytes, OK)
    }
}

/// Replica of gale_net_buf_push_decide.
fn ffi_net_buf_push_decide(head_offset: u16, len: u16, bytes: u16) -> (u16, u16, i32) {
    if bytes > head_offset {
        (head_offset, len, EINVAL)
    } else {
        (head_offset - bytes, len + bytes, OK)
    }
}

/// Replica of gale_net_buf_pull_decide.
fn ffi_net_buf_pull_decide(head_offset: u16, len: u16, _size: u16, bytes: u16) -> (u16, u16, i32) {
    if bytes > len {
        (head_offset, len, EINVAL)
    } else {
        (head_offset + bytes, len - bytes, OK)
    }
}

// =====================================================================
// Differential tests: alloc_decide
// =====================================================================

#[test]
fn net_buf_alloc_decide_ffi_matches_model_exhaustive() {
    for capacity in [1u16, 4, 16, 255, 1024] {
        for allocated in 0u16..=capacity {
            let (ffi_new, ffi_rc) = ffi_net_buf_alloc_decide(allocated, capacity);
            let model = alloc_decide(allocated, capacity);

            match model {
                Ok(model_new) => {
                    assert_eq!(ffi_rc, OK,
                        "alloc rc mismatch: allocated={allocated}, capacity={capacity}");
                    assert_eq!(ffi_new, model_new,
                        "alloc new_allocated mismatch: allocated={allocated}, capacity={capacity}");
                }
                Err(model_e) => {
                    assert_eq!(ffi_rc, model_e,
                        "alloc error mismatch: allocated={allocated}, capacity={capacity}");
                    assert_eq!(ffi_new, allocated,
                        "alloc unchanged on error: allocated={allocated}");
                }
            }
        }
    }
}

#[test]
fn net_buf_alloc_decide_full_pool_enomem() {
    let capacity = 8u16;
    let (_, rc) = ffi_net_buf_alloc_decide(capacity, capacity);
    assert_eq!(rc, ENOMEM, "NB1: full pool must return ENOMEM");

    let model = alloc_decide(capacity, capacity);
    assert_eq!(model, Err(ENOMEM));
}

// =====================================================================
// Differential tests: free_decide
// =====================================================================

#[test]
fn net_buf_free_decide_ffi_matches_model_exhaustive() {
    for allocated in 0u16..=32 {
        let ffi_rc = ffi_net_buf_free_decide(allocated);
        let model = free_decide(allocated);

        match model {
            Ok(_) => assert_eq!(ffi_rc, OK,
                "free rc mismatch: allocated={allocated}"),
            Err(model_e) => assert_eq!(ffi_rc, model_e,
                "free error mismatch: allocated={allocated}"),
        }
    }
}

#[test]
fn net_buf_free_decide_double_free_rejected() {
    // NB6: free on empty pool rejected
    let rc = ffi_net_buf_free_decide(0);
    assert_eq!(rc, EINVAL, "NB6: double-free must return EINVAL");

    let model = free_decide(0);
    assert_eq!(model, Err(EINVAL));
}

// =====================================================================
// Differential tests: ref_decide
// =====================================================================

#[test]
fn net_buf_ref_decide_ffi_matches_model_exhaustive() {
    for ref_count in 0u8..=254 {
        let (ffi_new, ffi_rc) = ffi_net_buf_ref_decide(ref_count);
        let model = ref_decide(ref_count);

        match model {
            Ok(model_new) => {
                assert_eq!(ffi_rc, OK,
                    "ref rc mismatch: ref_count={ref_count}");
                assert_eq!(ffi_new, model_new,
                    "ref new_ref_count mismatch: ref_count={ref_count}");
            }
            Err(model_e) => {
                assert_eq!(ffi_rc, model_e,
                    "ref error mismatch: ref_count={ref_count}");
            }
        }
    }
}

#[test]
fn net_buf_ref_decide_overflow_at_max() {
    let (_, rc) = ffi_net_buf_ref_decide(u8::MAX);
    assert_eq!(rc, EOVERFLOW, "NB3: ref at u8::MAX must overflow");

    let model = ref_decide(u8::MAX);
    assert_eq!(model, Err(EOVERFLOW));
}

// =====================================================================
// Differential tests: unref_decide
// =====================================================================

#[test]
fn net_buf_unref_decide_ffi_matches_model_exhaustive() {
    for ref_count in 0u8..=32 {
        let (ffi_new, ffi_free, ffi_rc) = ffi_net_buf_unref_decide(ref_count);
        let model = unref_decide(ref_count);

        match model {
            Ok((model_new, model_should_free)) => {
                assert_eq!(ffi_rc, OK,
                    "unref rc mismatch: ref_count={ref_count}");
                assert_eq!(ffi_new, model_new,
                    "unref new_ref mismatch: ref_count={ref_count}");
                assert_eq!(ffi_free, if model_should_free { 1 } else { 0 },
                    "unref should_free mismatch: ref_count={ref_count}");
            }
            Err(model_e) => {
                assert_eq!(ffi_rc, model_e,
                    "unref error mismatch: ref_count={ref_count}");
            }
        }
    }
}

#[test]
fn net_buf_unref_decide_last_ref_signals_free() {
    let (new_ref, should_free, rc) = ffi_net_buf_unref_decide(1);
    assert_eq!(rc, OK);
    assert_eq!(new_ref, 0);
    assert_eq!(should_free, 1, "NB3/NB6: unref from 1 must signal free");

    let model = unref_decide(1);
    assert_eq!(model, Ok((0u8, true)));
}

#[test]
fn net_buf_unref_decide_double_free_rejected() {
    let (_, _, rc) = ffi_net_buf_unref_decide(0);
    assert_eq!(rc, EINVAL, "NB6: double-unref must return EINVAL");

    let model = unref_decide(0);
    assert_eq!(model, Err(EINVAL));
}

// =====================================================================
// Differential tests: add_decide (tail append)
// =====================================================================

#[test]
fn net_buf_add_decide_ffi_matches_model_exhaustive() {
    let size = 64u16;
    for head_offset in [0u16, 4, 16, 32] {
        for len in 0u16..=(size - head_offset) {
            for bytes in [0u16, 1, 4, 16, 32, 64] {
                let (ffi_head, ffi_new_len, ffi_rc) =
                    ffi_net_buf_add_decide(head_offset, len, size, bytes);
                let model = add_decide(head_offset, len, size, bytes);

                match model {
                    Ok(model_new_len) => {
                        assert_eq!(ffi_rc, OK,
                            "add rc: ho={head_offset}, len={len}, size={size}, bytes={bytes}");
                        assert_eq!(ffi_new_len, model_new_len,
                            "add new_len: ho={head_offset}, len={len}, size={size}, bytes={bytes}");
                        assert_eq!(ffi_head, head_offset,
                            "add head_offset must be unchanged");
                    }
                    Err(model_e) => {
                        assert_eq!(ffi_rc, model_e,
                            "add error: ho={head_offset}, len={len}, size={size}, bytes={bytes}");
                    }
                }
            }
        }
    }
}

// =====================================================================
// Differential tests: remove_decide (tail shrink)
// =====================================================================

#[test]
fn net_buf_remove_decide_ffi_matches_model_exhaustive() {
    for len in 0u16..=64 {
        for bytes in [0u16, 1, 8, 32, 64, 100] {
            let (ffi_head, ffi_new_len, ffi_rc) =
                ffi_net_buf_remove_decide(16, len, bytes);
            let model = remove_decide(len, bytes);

            match model {
                Ok(model_new_len) => {
                    assert_eq!(ffi_rc, OK,
                        "remove rc: len={len}, bytes={bytes}");
                    assert_eq!(ffi_new_len, model_new_len,
                        "remove new_len: len={len}, bytes={bytes}");
                    assert_eq!(ffi_head, 16u16,
                        "remove head_offset must be unchanged");
                }
                Err(model_e) => {
                    assert_eq!(ffi_rc, model_e,
                        "remove error: len={len}, bytes={bytes}");
                }
            }
        }
    }
}

// =====================================================================
// Differential tests: push_decide (prepend at head)
// =====================================================================

#[test]
fn net_buf_push_decide_ffi_matches_model_exhaustive() {
    for head_offset in [0u16, 4, 16, 32] {
        for len in [0u16, 4, 16] {
            for bytes in [0u16, 1, 4, 16, 32, 64] {
                let (ffi_new_head, ffi_new_len, ffi_rc) =
                    ffi_net_buf_push_decide(head_offset, len, bytes);
                let model = push_decide(head_offset, len, bytes);

                match model {
                    Ok((model_new_head, model_new_len)) => {
                        assert_eq!(ffi_rc, OK,
                            "push rc: ho={head_offset}, len={len}, bytes={bytes}");
                        assert_eq!(ffi_new_head, model_new_head,
                            "push new_head: ho={head_offset}, len={len}, bytes={bytes}");
                        assert_eq!(ffi_new_len, model_new_len,
                            "push new_len: ho={head_offset}, len={len}, bytes={bytes}");
                    }
                    Err(model_e) => {
                        assert_eq!(ffi_rc, model_e,
                            "push error: ho={head_offset}, len={len}, bytes={bytes}");
                    }
                }
            }
        }
    }
}

// =====================================================================
// Differential tests: pull_decide (consume from head)
// =====================================================================

#[test]
fn net_buf_pull_decide_ffi_matches_model_exhaustive() {
    let size = 128u16;
    for head_offset in [0u16, 4, 16, 32] {
        for len in [0u16, 4, 16, 32] {
            for bytes in [0u16, 1, 4, 16, 32, 64] {
                let (ffi_new_head, ffi_new_len, ffi_rc) =
                    ffi_net_buf_pull_decide(head_offset, len, size, bytes);
                let model = pull_decide(head_offset, len, size, bytes);

                match model {
                    Ok((model_new_head, model_new_len)) => {
                        assert_eq!(ffi_rc, OK,
                            "pull rc: ho={head_offset}, len={len}, bytes={bytes}");
                        assert_eq!(ffi_new_head, model_new_head,
                            "pull new_head: ho={head_offset}, len={len}, bytes={bytes}");
                        assert_eq!(ffi_new_len, model_new_len,
                            "pull new_len: ho={head_offset}, len={len}, bytes={bytes}");
                    }
                    Err(model_e) => {
                        assert_eq!(ffi_rc, model_e,
                            "pull error: ho={head_offset}, len={len}, bytes={bytes}");
                    }
                }
            }
        }
    }
}

// =====================================================================
// Property: NB5 — push-pull roundtrip restores original state
// =====================================================================

#[test]
fn net_buf_push_pull_roundtrip() {
    let size = 128u16;
    for head_offset in [4u16, 8, 16, 32] {
        for len in [0u16, 4, 8, 16] {
            for bytes in [1u16, 2, 4, 8] {
                let push = push_decide(head_offset, len, bytes);
                if let Ok((new_head, new_len)) = push {
                    let pull = pull_decide(new_head, new_len, size, bytes);
                    if let Ok((restored_head, restored_len)) = pull {
                        assert_eq!(restored_head, head_offset,
                            "NB5: push-pull should restore head_offset");
                        assert_eq!(restored_len, len,
                            "NB5: push-pull should restore len");
                    }
                }
            }
        }
    }
}

// =====================================================================
// Property: NB1 — alloc-free roundtrip restores count
// =====================================================================

#[test]
fn net_buf_alloc_free_roundtrip() {
    for initial in 0u16..=16 {
        let capacity = 32u16;
        let alloc = alloc_decide(initial, capacity);
        if let Ok(after_alloc) = alloc {
            let free = free_decide(after_alloc);
            if let Ok(after_free) = free {
                assert_eq!(after_free, initial,
                    "NB1/NB2: alloc-free roundtrip failed: initial={initial}");
            }
        }
    }
}
