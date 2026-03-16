//! Integration tests for the ring buffer model.
//!
//! Mirrors Zephyr's tests/lib/ring_buffer test cases.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::shadow_unrelated
)]

use gale::error::*;
use gale::ring_buf::RingBuf;

// =========================================================================
// Init validation
// =========================================================================

#[test]
fn rb_init_valid_capacity() {
    let rb = RingBuf::init(16).unwrap();
    assert_eq!(rb.capacity_get(), 16);
    assert_eq!(rb.size_get(), 0);
    assert_eq!(rb.space_get(), 16);
    assert!(rb.is_empty());
    assert!(!rb.is_full());
    assert_eq!(rb.head_get(), 0);
    assert_eq!(rb.tail_get(), 0);
}

#[test]
fn rb_init_rejects_zero_capacity() {
    assert_eq!(RingBuf::init(0).unwrap_err(), EINVAL);
}

#[test]
fn rb_init_capacity_one() {
    let rb = RingBuf::init(1).unwrap();
    assert_eq!(rb.capacity_get(), 1);
    assert!(rb.is_empty());
}

#[test]
fn rb_init_max_capacity() {
    let rb = RingBuf::init(u32::MAX).unwrap();
    assert_eq!(rb.capacity_get(), u32::MAX);
    assert!(rb.is_empty());
}

// =========================================================================
// RB3: Put operations (tail advances)
// =========================================================================

#[test]
fn rb_put_sequential() {
    let mut rb = RingBuf::init(4).unwrap();
    for i in 0..4 {
        let slot = rb.put().unwrap();
        assert_eq!(slot, i);
        assert_eq!(rb.size_get(), i + 1);
    }
    assert!(rb.is_full());
}

#[test]
fn rb5_put_full_returns_eagain() {
    let mut rb = RingBuf::init(2).unwrap();
    rb.put().unwrap();
    rb.put().unwrap();
    assert_eq!(rb.put().unwrap_err(), EAGAIN);
    assert_eq!(rb.size_get(), 2); // unchanged
}

#[test]
fn rb3_put_wraps_tail() {
    let mut rb = RingBuf::init(3).unwrap();
    rb.put().unwrap(); // slot 0
    rb.put().unwrap(); // slot 1
    rb.put().unwrap(); // slot 2
    rb.get().unwrap(); // free slot 0, head=1
    assert_eq!(rb.put().unwrap(), 0); // tail wraps to 0
}

#[test]
fn rb_put_capacity_one() {
    let mut rb = RingBuf::init(1).unwrap();
    assert_eq!(rb.put().unwrap(), 0);
    assert!(rb.is_full());
    assert_eq!(rb.put().unwrap_err(), EAGAIN);
}

// =========================================================================
// RB4: Get operations (head advances)
// =========================================================================

#[test]
fn rb6_get_empty_returns_eagain() {
    let mut rb = RingBuf::init(3).unwrap();
    assert_eq!(rb.get().unwrap_err(), EAGAIN);
}

#[test]
fn rb4_get_fifo_order() {
    let mut rb = RingBuf::init(5).unwrap();
    for _ in 0..5 {
        rb.put().unwrap();
    }
    for i in 0..5 {
        assert_eq!(rb.get().unwrap(), i);
    }
}

#[test]
fn rb4_get_wraps_head() {
    let mut rb = RingBuf::init(3).unwrap();
    // Fill and partially drain
    rb.put().unwrap();
    rb.put().unwrap();
    rb.put().unwrap();
    rb.get().unwrap(); // slot 0
    rb.get().unwrap(); // slot 1
    // Refill
    rb.put().unwrap(); // slot 0 (wrapped)
    rb.put().unwrap(); // slot 1 (wrapped)
    // Continue reading
    assert_eq!(rb.get().unwrap(), 2);
    assert_eq!(rb.get().unwrap(), 0); // wrapped
    assert_eq!(rb.get().unwrap(), 1); // wrapped
}

#[test]
fn rb_get_capacity_one() {
    let mut rb = RingBuf::init(1).unwrap();
    rb.put().unwrap();
    assert_eq!(rb.get().unwrap(), 0);
    assert!(rb.is_empty());
    assert_eq!(rb.get().unwrap_err(), EAGAIN);
}

// =========================================================================
// Multi-byte put_n / get_n
// =========================================================================

#[test]
fn rb_put_n_full_write() {
    let mut rb = RingBuf::init(8).unwrap();
    let written = rb.put_n(8);
    assert_eq!(written, 8);
    assert!(rb.is_full());
    assert_eq!(rb.tail_get(), 0); // wrapped back to 0
}

#[test]
fn rb_put_n_partial_write() {
    let mut rb = RingBuf::init(5).unwrap();
    rb.put_n(3);
    let written = rb.put_n(5); // only 2 free
    assert_eq!(written, 2);
    assert!(rb.is_full());
}

#[test]
fn rb_put_n_zero_count() {
    let mut rb = RingBuf::init(4).unwrap();
    let written = rb.put_n(0);
    assert_eq!(written, 0);
    assert!(rb.is_empty());
}

#[test]
fn rb_get_n_full_read() {
    let mut rb = RingBuf::init(8).unwrap();
    rb.put_n(8);
    let read = rb.get_n(8);
    assert_eq!(read, 8);
    assert!(rb.is_empty());
}

#[test]
fn rb_get_n_partial_read() {
    let mut rb = RingBuf::init(5).unwrap();
    rb.put_n(3);
    let read = rb.get_n(5); // only 3 available
    assert_eq!(read, 3);
    assert!(rb.is_empty());
}

#[test]
fn rb_get_n_zero_count() {
    let mut rb = RingBuf::init(4).unwrap();
    rb.put_n(2);
    let read = rb.get_n(0);
    assert_eq!(read, 0);
    assert_eq!(rb.size_get(), 2);
}

#[test]
fn rb_put_n_get_n_wrapping() {
    let mut rb = RingBuf::init(4).unwrap();
    rb.put_n(3); // tail at 3
    rb.get_n(2); // head at 2
    let written = rb.put_n(3); // 3 free, wraps tail around
    assert_eq!(written, 3);
    assert_eq!(rb.tail_get(), 2); // (3+3) % 4 = 2
    assert_eq!(rb.size_get(), 4); // full
}

// =========================================================================
// Peek
// =========================================================================

#[test]
fn rb_peek_at_sequential() {
    let mut rb = RingBuf::init(5).unwrap();
    rb.put().unwrap();
    rb.put().unwrap();
    rb.put().unwrap();

    assert_eq!(rb.peek_at(0).unwrap(), 0);
    assert_eq!(rb.peek_at(1).unwrap(), 1);
    assert_eq!(rb.peek_at(2).unwrap(), 2);
}

#[test]
fn rb_peek_at_with_wrap() {
    let mut rb = RingBuf::init(3).unwrap();
    rb.put().unwrap();
    rb.put().unwrap();
    rb.get().unwrap(); // head = 1
    rb.get().unwrap(); // head = 2
    rb.put().unwrap(); // slot 2
    rb.put().unwrap(); // slot 0
    rb.put().unwrap(); // slot 1

    assert_eq!(rb.peek_at(0).unwrap(), 2); // head = 2
    assert_eq!(rb.peek_at(1).unwrap(), 0); // wraps
    assert_eq!(rb.peek_at(2).unwrap(), 1); // wraps
}

#[test]
fn rb_peek_at_out_of_bounds() {
    let mut rb = RingBuf::init(5).unwrap();
    rb.put().unwrap();
    assert_eq!(rb.peek_at(1).unwrap_err(), EINVAL);
}

#[test]
fn rb_peek_at_empty() {
    let rb = RingBuf::init(5).unwrap();
    assert_eq!(rb.peek_at(0).unwrap_err(), EINVAL);
}

#[test]
fn rb_peek_does_not_consume() {
    let mut rb = RingBuf::init(3).unwrap();
    rb.put().unwrap();
    rb.put().unwrap();
    rb.peek_at(0).unwrap();
    rb.peek_at(1).unwrap();
    assert_eq!(rb.size_get(), 2); // unchanged
}

// =========================================================================
// Reset
// =========================================================================

#[test]
fn rb_reset_empties_buffer() {
    let mut rb = RingBuf::init(5).unwrap();
    rb.put().unwrap();
    rb.put().unwrap();
    rb.put().unwrap();
    rb.reset();
    assert!(rb.is_empty());
    assert_eq!(rb.size_get(), 0);
    assert_eq!(rb.head_get(), 0);
    assert_eq!(rb.tail_get(), 0);
}

#[test]
fn rb_reset_idempotent() {
    let mut rb = RingBuf::init(3).unwrap();
    rb.reset();
    rb.reset();
    assert!(rb.is_empty());
}

#[test]
fn rb_reset_allows_reuse() {
    let mut rb = RingBuf::init(3).unwrap();
    rb.put().unwrap();
    rb.put().unwrap();
    rb.reset();

    // Should be able to put again
    rb.put().unwrap();
    assert_eq!(rb.size_get(), 1);

    // And get
    rb.get().unwrap();
    assert!(rb.is_empty());
}

// =========================================================================
// RB7: Ring consistency across operations
// =========================================================================

#[test]
fn rb7_ring_consistency_across_operations() {
    let mut rb = RingBuf::init(4).unwrap();

    let check_ring = |rb: &RingBuf| {
        let expected = (rb.head_get() as u64 + rb.size_get() as u64) % rb.capacity_get() as u64;
        assert_eq!(
            rb.tail_get() as u64,
            expected,
            "ring inconsistency: h={} s={} t={} cap={}",
            rb.head_get(),
            rb.size_get(),
            rb.tail_get(),
            rb.capacity_get()
        );
    };

    check_ring(&rb);
    for _ in 0..4 {
        rb.put().unwrap();
        check_ring(&rb);
    }
    for _ in 0..4 {
        rb.get().unwrap();
        check_ring(&rb);
    }
    rb.put().unwrap();
    check_ring(&rb);
    rb.get().unwrap();
    check_ring(&rb);
    rb.reset();
    check_ring(&rb);
}

#[test]
fn rb1_size_plus_space_invariant() {
    let mut rb = RingBuf::init(5).unwrap();
    let cap = rb.capacity_get();
    assert_eq!(rb.size_get() + rb.space_get(), cap);

    rb.put().unwrap();
    assert_eq!(rb.size_get() + rb.space_get(), cap);

    rb.put().unwrap();
    assert_eq!(rb.size_get() + rb.space_get(), cap);

    rb.get().unwrap();
    assert_eq!(rb.size_get() + rb.space_get(), cap);

    rb.reset();
    assert_eq!(rb.size_get() + rb.space_get(), cap);
}

// =========================================================================
// Stress tests
// =========================================================================

#[test]
fn rb_stress_fill_drain_cycles() {
    let mut rb = RingBuf::init(8).unwrap();
    for _ in 0..100 {
        // Fill
        while rb.put().is_ok() {}
        assert!(rb.is_full());
        // Drain
        while rb.get().is_ok() {}
        assert!(rb.is_empty());
    }
}

#[test]
fn rb_stress_interleaved_put_get() {
    let mut rb = RingBuf::init(3).unwrap();

    for _ in 0..200 {
        // Put 2
        for _ in 0..2 {
            let _ = rb.put();
        }
        // Get 1
        let _ = rb.get();
        // Invariants
        assert!(rb.size_get() <= rb.capacity_get());
        let expected = (rb.head_get() as u64 + rb.size_get() as u64) % rb.capacity_get() as u64;
        assert_eq!(rb.tail_get() as u64, expected);
    }
}

#[test]
fn rb_stress_put_n_get_n_cycles() {
    let mut rb = RingBuf::init(16).unwrap();
    for cycle in 0..50 {
        let chunk = (cycle % 16) + 1;
        let written = rb.put_n(chunk);
        assert!(written <= chunk);
        let read = rb.get_n(written);
        assert_eq!(read, written);
    }
    // Should end empty since we read back everything we wrote
    assert!(rb.is_empty());
}

#[test]
fn rb8_no_overflow_near_max_capacity() {
    // Test with a large capacity to exercise u64 arithmetic paths
    let mut rb = RingBuf::init(u32::MAX).unwrap();
    rb.put().unwrap();
    rb.get().unwrap();
    assert!(rb.is_empty());
}

#[test]
fn rb_put_get_alternating_single() {
    let mut rb = RingBuf::init(1).unwrap();
    for _ in 0..100 {
        rb.put().unwrap();
        assert!(rb.is_full());
        rb.get().unwrap();
        assert!(rb.is_empty());
    }
}
