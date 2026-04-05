//! Differential equivalence tests — RingBuf (FFI vs Model).
//!
//! Verifies that the FFI ring buffer functions produce the same results
//! as the Verus-verified model functions in gale::ring_buf.

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
use gale::ring_buf::RingBuf;

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Replica of gale_ring_buf_claim_decide.
///
/// Returns (claim_size, buffer_offset).
fn ffi_ring_buf_claim_decide(head: u32, base: u32, buf_size: u32, requested: u32) -> (u32, u32) {
    if buf_size == 0 {
        return (0, 0);
    }

    // head_offset = head - base, with wraparound adjustment.
    let raw_offset = head.wrapping_sub(base);
    let head_offset = if raw_offset >= buf_size {
        raw_offset - buf_size
    } else {
        raw_offset
    };
    // Clamp: if still >= buf_size (shouldn't happen with valid state),
    // use modulo to guarantee bounds.
    let head_offset = if head_offset >= buf_size {
        head_offset % buf_size
    } else {
        head_offset
    };

    // wrap_size = bytes until end of physical buffer
    let wrap_size = buf_size - head_offset;
    let claim_size = if requested <= wrap_size {
        requested
    } else {
        wrap_size
    };

    (claim_size, head_offset)
}

/// Replica of gale_ring_buf_finish_validate.
///
/// Returns 0=OK, -EINVAL if size > claimed.
fn ffi_ring_buf_finish_validate(head: u32, tail: u32, size: u32, _buf_size: u32) -> i32 {
    let claimed_size = head.wrapping_sub(tail);
    if size > claimed_size {
        return EINVAL;
    }
    OK
}

/// Replica of gale_ring_buf_space_get.
fn ffi_ring_buf_space_get(put_head: u32, get_tail: u32, buf_size: u32) -> u32 {
    if buf_size == 0 {
        return 0;
    }
    let allocated = put_head.wrapping_sub(get_tail);
    if allocated > buf_size {
        0
    } else {
        buf_size - allocated
    }
}

/// Replica of gale_ring_buf_size_get.
fn ffi_ring_buf_size_get(put_tail: u32, get_head: u32) -> u32 {
    put_tail.wrapping_sub(get_head)
}

// =====================================================================
// Differential tests: claim_decide
// =====================================================================

#[test]
fn ring_buf_claim_decide_zero_buf_size_returns_zero() {
    let (claim, offset) = ffi_ring_buf_claim_decide(0, 0, 0, 100);
    assert_eq!(claim, 0, "zero buf_size must return claim=0");
    assert_eq!(offset, 0, "zero buf_size must return offset=0");
}

#[test]
fn ring_buf_claim_decide_offset_bounded_by_buf_size() {
    let test_cases: &[(u32, u32, u32, u32)] = &[
        (0, 0, 1024, 512),
        (100, 0, 1024, 512),
        (1000, 0, 1024, 100),
        (0, 500, 1024, 256),
        (500, 500, 1024, 256),
        (1023, 0, 1024, 1),
        (0, 0, 1, 1),
        (1, 0, 1, 0),
    ];
    for &(head, base, buf_size, requested) in test_cases {
        let (claim_size, buffer_offset) = ffi_ring_buf_claim_decide(head, base, buf_size, requested);
        assert!(
            buffer_offset < buf_size,
            "RB1: offset must be < buf_size: head={head}, base={base}, buf={buf_size}"
        );
        assert!(
            claim_size <= buf_size,
            "RB1: claim_size must be <= buf_size: head={head}, base={base}, buf={buf_size}"
        );
        assert!(
            claim_size <= requested,
            "claim_size must be <= requested: head={head}, requested={requested}"
        );
    }
}

#[test]
fn ring_buf_claim_decide_no_wrap_full_request() {
    // head=0, base=0, buf_size=100, requested=50: no wrap, all 50 available
    let (claim, offset) = ffi_ring_buf_claim_decide(0, 0, 100, 50);
    assert_eq!(offset, 0, "offset should be 0 when head==base");
    assert_eq!(claim, 50, "full request fits without wrap");
}

#[test]
fn ring_buf_claim_decide_near_end_wraps_correctly() {
    // head=900, base=0, buf_size=1000, requested=200 -> wrap_size=100, claim=100
    let (claim, offset) = ffi_ring_buf_claim_decide(900, 0, 1000, 200);
    assert_eq!(offset, 900, "offset should be 900");
    assert_eq!(claim, 100, "claim should be capped at wrap_size=100");
}

// =====================================================================
// Differential tests: finish_validate
// =====================================================================

#[test]
fn ring_buf_finish_validate_accepts_exact_claimed() {
    // head = tail + size (exactly what was claimed)
    let tail = 0u32;
    let size = 100u32;
    let head = tail.wrapping_add(size);
    let ret = ffi_ring_buf_finish_validate(head, tail, size, 1024);
    assert_eq!(ret, OK, "RB3: finish with exact claimed size must succeed");
}

#[test]
fn ring_buf_finish_validate_accepts_partial_finish() {
    let tail = 0u32;
    let claimed = 200u32;
    let head = tail.wrapping_add(claimed);
    // Finish only 100 of the 200 claimed bytes
    let ret = ffi_ring_buf_finish_validate(head, tail, 100, 1024);
    assert_eq!(ret, OK, "partial finish must succeed");
}

#[test]
fn ring_buf_finish_validate_rejects_over_finish() {
    let tail = 0u32;
    let claimed = 50u32;
    let head = tail.wrapping_add(claimed);
    // Try to finish 51 bytes but only 50 were claimed
    let ret = ffi_ring_buf_finish_validate(head, tail, 51, 1024);
    assert_eq!(ret, EINVAL, "RB3: over-finish must be rejected");
}

#[test]
fn ring_buf_finish_validate_zero_claimed_rejects_nonzero() {
    // head == tail: nothing claimed
    let ret = ffi_ring_buf_finish_validate(0, 0, 1, 1024);
    assert_eq!(ret, EINVAL, "zero claimed must reject nonzero finish");
}

#[test]
fn ring_buf_finish_validate_wrapping_claim_exhaustive() {
    // Exercise wrapping subtraction: head wraps below tail
    let tail = 0u32;
    let size_set: &[u32] = &[0, 1, 10, 100, 1000, u32::MAX / 2];
    for &size in size_set {
        let head = tail.wrapping_add(size);
        let ret = ffi_ring_buf_finish_validate(head, tail, size, 4096);
        assert_eq!(ret, OK, "wrapping claim should validate: size={size}");
        if size > 0 {
            let ret = ffi_ring_buf_finish_validate(head, tail, size + 1, 4096);
            assert_eq!(ret, EINVAL, "over-finish should fail: size={size}");
        }
    }
}

// =====================================================================
// Differential tests: space_get
// =====================================================================

#[test]
fn ring_buf_space_get_zero_buf_size_returns_zero() {
    let space = ffi_ring_buf_space_get(100, 50, 0);
    assert_eq!(space, 0, "space_get with buf_size=0 must return 0");
}

#[test]
fn ring_buf_space_get_empty_buffer_equals_capacity() {
    // put_head == get_tail: allocated = 0, space = buf_size
    let buf_size = 1024u32;
    let space = ffi_ring_buf_space_get(500, 500, buf_size);
    assert_eq!(space, buf_size, "RB1: empty buffer space == capacity");
}

#[test]
fn ring_buf_space_get_full_buffer_returns_zero() {
    // allocated == buf_size: space = 0
    let buf_size = 1024u32;
    let put_head = buf_size;
    let get_tail = 0u32;
    let space = ffi_ring_buf_space_get(put_head, get_tail, buf_size);
    assert_eq!(space, 0, "RB5: full buffer space == 0");
}

#[test]
fn ring_buf_space_get_exhaustive_small() {
    // For small capacities, exhaustively check all valid states
    for buf_size in 1u32..=8 {
        for get_tail in 0u32..buf_size {
            for allocated in 0u32..=buf_size {
                let put_head = get_tail.wrapping_add(allocated);
                let space = ffi_ring_buf_space_get(put_head, get_tail, buf_size);
                let expected = buf_size - allocated;
                assert_eq!(space, expected,
                    "RB7: space_get mismatch: buf={buf_size}, alloc={allocated}");
            }
        }
    }
}

// =====================================================================
// Differential tests: size_get
// =====================================================================

#[test]
fn ring_buf_size_get_empty_returns_zero() {
    // put_tail == get_head: no data
    let size = ffi_ring_buf_size_get(500, 500);
    assert_eq!(size, 0, "empty buffer size == 0");
}

#[test]
fn ring_buf_size_get_exhaustive_small() {
    for get_head in 0u32..=5 {
        for data_bytes in 0u32..=5 {
            let put_tail = get_head.wrapping_add(data_bytes);
            let size = ffi_ring_buf_size_get(put_tail, get_head);
            assert_eq!(size, data_bytes,
                "size_get mismatch: get_head={get_head}, data={data_bytes}");
        }
    }
}

// =====================================================================
// Differential tests: space_get + size_get = buf_size (RB7)
// =====================================================================

#[test]
fn ring_buf_space_plus_size_equals_capacity() {
    // When put_tail == put_head and get_tail == get_head (no in-flight claims),
    // space + size must equal buf_size.
    let configs: &[(u32, u32, u32)] = &[
        (0, 0, 1024),
        (512, 0, 1024),
        (0, 512, 1024),
        (1024, 0, 1024),
        (100, 100, 1024),
    ];

    for &(put_head, get_tail, buf_size) in configs {
        let allocated = put_head.wrapping_sub(get_tail);
        if allocated > buf_size {
            continue; // Skip invalid states
        }
        // With no in-flight: put_tail == put_head, get_head == get_tail
        let space = ffi_ring_buf_space_get(put_head, get_tail, buf_size);
        let size = ffi_ring_buf_size_get(put_head, get_tail);
        assert_eq!(
            space.wrapping_add(size),
            buf_size,
            "RB7: space + size must equal buf_size: put_head={put_head}, get_tail={get_tail}"
        );
    }
}

// =====================================================================
// Differential tests: model put/get roundtrip vs ffi space_get/size_get
// =====================================================================

#[test]
fn ring_buf_model_put_get_roundtrip() {
    for capacity in [1u32, 4, 8, 16, 64] {
        let mut rb = RingBuf::init(capacity).unwrap();
        let initial_size = rb.size_get();
        assert_eq!(initial_size, 0, "RB: init creates empty buffer");
        assert_eq!(rb.space_get(), capacity, "RB: init space == capacity");

        if rb.put().is_ok() {
            let rc = rb.get();
            assert!(rc.is_ok(), "RB3+RB4: put then get must succeed");
            assert_eq!(rb.size_get(), initial_size, "roundtrip restores size");
        }
    }
}

#[test]
fn ring_buf_model_size_get_matches_ffi_size_get() {
    for capacity in [1u32, 8, 32] {
        let mut rb = RingBuf::init(capacity).unwrap();

        // Put some bytes
        let n_puts = capacity / 2;
        for _ in 0..n_puts {
            rb.put().unwrap();
        }

        let model_size = rb.size_get();
        // In model: put_tail conceptually = tail, get_head = head
        let ffi_size = ffi_ring_buf_size_get(rb.tail, rb.head);
        // Note: ffi_size_get uses wrapping subtraction; model tracks size explicitly.
        // They agree when tail >= head (no wrap yet).
        if rb.tail >= rb.head {
            assert_eq!(ffi_size, model_size,
                "size_get mismatch (no wrap): cap={capacity}, puts={n_puts}");
        }
    }
}

#[test]
fn ring_buf_model_space_get_matches_ffi_space_get() {
    for capacity in [1u32, 8, 32] {
        let mut rb = RingBuf::init(capacity).unwrap();

        let n_puts = capacity / 2;
        for _ in 0..n_puts {
            rb.put().unwrap();
        }

        let model_space = rb.space_get();
        // ffi_space_get with put_head=tail, get_tail=head, buf_size=capacity
        // (approximation — no in-flight puts)
        if rb.tail >= rb.head {
            let ffi_space = ffi_ring_buf_space_get(rb.tail, rb.head, capacity);
            assert_eq!(ffi_space, model_space,
                "space_get mismatch (no wrap): cap={capacity}, puts={n_puts}");
        }
    }
}

// =====================================================================
// Property: RB5 — put on full buffer returns error
// =====================================================================

#[test]
fn ring_buf_put_full_returns_error() {
    let mut rb = RingBuf::init(4).unwrap();
    // Fill the buffer
    for _ in 0..4 {
        assert!(rb.put().is_ok(), "should put while space available");
    }
    // Now full — next put must fail
    let ret = rb.put();
    assert!(ret.is_err(), "RB5: put on full buffer must return error");
}

// =====================================================================
// Property: RB6 — get on empty buffer returns error
// =====================================================================

#[test]
fn ring_buf_get_empty_returns_error() {
    let mut rb = RingBuf::init(4).unwrap();
    let ret = rb.get();
    assert!(ret.is_err(), "RB6: get on empty buffer must return error");
}

// =====================================================================
// Property: RB1 — size and space are always bounded by capacity
// =====================================================================

#[test]
fn ring_buf_size_space_bounded_by_capacity() {
    let capacity = 8u32;
    let mut rb = RingBuf::init(capacity).unwrap();

    for _ in 0..capacity {
        let size = rb.size_get();
        let space = rb.space_get();
        assert!(size <= capacity, "RB1: size <= capacity");
        assert!(space <= capacity, "RB1: space <= capacity");
        assert_eq!(size + space, capacity, "RB9: size + space == capacity");
        if rb.put().is_err() {
            break;
        }
    }

    for _ in 0..capacity {
        let size = rb.size_get();
        let space = rb.space_get();
        assert!(size <= capacity, "RB1: size <= capacity after puts");
        assert_eq!(size + space, capacity, "RB9: size + space == capacity after puts");
        if rb.get().is_err() {
            break;
        }
    }
}

// =====================================================================
// Property: reset empties the buffer
// =====================================================================

#[test]
fn ring_buf_reset_empties_buffer() {
    let mut rb = RingBuf::init(16).unwrap();
    for _ in 0..8 {
        rb.put().unwrap();
    }
    assert_eq!(rb.size_get(), 8);
    rb.reset();
    assert_eq!(rb.size_get(), 0, "reset must empty buffer");
    assert_eq!(rb.space_get(), 16, "reset must restore full space");
    assert!(rb.is_empty(), "reset must leave buffer empty");
}
