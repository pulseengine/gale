//! Property-based tests for the ring buffer model.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::shadow_unrelated,
    clippy::wildcard_enum_match_arm,
    clippy::unreachable
)]

use gale::error::*;
use gale::ring_buf::RingBuf;
use proptest::prelude::*;

/// Strategy for valid ring buffer capacity.
fn valid_capacity() -> impl Strategy<Value = u32> {
    1..=1024u32
}

proptest! {
    /// RB1: init with valid capacity always succeeds.
    #[test]
    fn prop_init_valid_capacity(cap in valid_capacity()) {
        let rb = RingBuf::init(cap).unwrap();
        prop_assert_eq!(rb.capacity_get(), cap);
        prop_assert_eq!(rb.size_get(), 0);
        prop_assert_eq!(rb.space_get(), cap);
        prop_assert!(rb.is_empty());
        prop_assert!(!rb.is_full());
    }

    /// RB1: size + space == capacity after arbitrary operations.
    #[test]
    fn prop_conservation(
        cap in valid_capacity(),
        ops in proptest::collection::vec(0..4u8, 1..100)
    ) {
        let mut rb = RingBuf::init(cap).unwrap();

        for op in ops {
            match op {
                0 => { let _ = rb.put(); }
                1 => { let _ = rb.get(); }
                2 => { rb.reset(); }
                _ => { let _ = rb.peek_at(0); }
            }
            prop_assert_eq!(
                rb.size_get() + rb.space_get(),
                rb.capacity_get(),
                "conservation violated: size={} space={} cap={}",
                rb.size_get(), rb.space_get(), rb.capacity_get()
            );
        }
    }

    /// RB2: head and tail are always < capacity.
    #[test]
    fn prop_index_bounds(
        cap in valid_capacity(),
        ops in proptest::collection::vec(0..3u8, 1..100)
    ) {
        let mut rb = RingBuf::init(cap).unwrap();

        for op in ops {
            match op {
                0 => { let _ = rb.put(); }
                1 => { let _ = rb.get(); }
                _ => { rb.reset(); }
            }
            prop_assert!(rb.head_get() < rb.capacity_get(),
                "head {} >= cap {}", rb.head_get(), rb.capacity_get());
            prop_assert!(rb.tail_get() < rb.capacity_get(),
                "tail {} >= cap {}", rb.tail_get(), rb.capacity_get());
        }
    }

    /// RB3/RB4: put returns sequential slots, get returns FIFO order.
    #[test]
    fn prop_fifo_order(cap in 1..=64u32) {
        let mut rb = RingBuf::init(cap).unwrap();

        // Fill
        for i in 0..cap {
            let slot = rb.put().unwrap();
            prop_assert_eq!(slot, i);
        }

        // Drain — FIFO order
        for i in 0..cap {
            let slot = rb.get().unwrap();
            prop_assert_eq!(slot, i);
        }
    }

    /// RB5: put on full returns EAGAIN.
    #[test]
    fn prop_put_full_fails(cap in valid_capacity()) {
        let mut rb = RingBuf::init(cap).unwrap();
        // Fill
        for _ in 0..cap {
            rb.put().unwrap();
        }
        prop_assert!(rb.is_full());
        prop_assert_eq!(rb.put().unwrap_err(), EAGAIN);
        prop_assert_eq!(rb.size_get(), cap); // unchanged
    }

    /// RB6: get on empty returns EAGAIN.
    #[test]
    fn prop_get_empty_fails(cap in valid_capacity()) {
        let mut rb = RingBuf::init(cap).unwrap();
        prop_assert!(rb.is_empty());
        prop_assert_eq!(rb.get().unwrap_err(), EAGAIN);
        prop_assert_eq!(rb.size_get(), 0); // unchanged
    }

    /// RB7: ring consistency after arbitrary operation sequences.
    #[test]
    fn prop_ring_consistency(
        cap in valid_capacity(),
        ops in proptest::collection::vec(0..4u8, 1..100)
    ) {
        let mut rb = RingBuf::init(cap).unwrap();

        for op in ops {
            match op {
                0 => { let _ = rb.put(); }
                1 => { let _ = rb.get(); }
                2 => { rb.reset(); }
                _ => { let _ = rb.peek_at(0); }
            }
            let expected_tail =
                (rb.head_get() as u64 + rb.size_get() as u64) % rb.capacity_get() as u64;
            prop_assert_eq!(
                rb.tail_get() as u64,
                expected_tail,
                "ring inconsistency: h={} s={} t={} cap={}",
                rb.head_get(), rb.size_get(), rb.tail_get(), rb.capacity_get()
            );
        }
    }

    /// RB3: put advances tail.
    #[test]
    fn prop_put_advances_tail(cap in 2..=256u32) {
        let mut rb = RingBuf::init(cap).unwrap();
        let old_tail = rb.tail_get();
        rb.put().unwrap();
        let new_tail = rb.tail_get();
        let expected = if old_tail + 1 < cap { old_tail + 1 } else { 0 };
        prop_assert_eq!(new_tail, expected);
    }

    /// RB4: get advances head.
    #[test]
    fn prop_get_advances_head(cap in 2..=256u32) {
        let mut rb = RingBuf::init(cap).unwrap();
        rb.put().unwrap();
        let old_head = rb.head_get();
        rb.get().unwrap();
        let new_head = rb.head_get();
        let expected = if old_head + 1 < cap { old_head + 1 } else { 0 };
        prop_assert_eq!(new_head, expected);
    }

    /// Fill-drain preserves invariants throughout.
    #[test]
    fn prop_fill_drain_preserves_invariants(cap in 1..=128u32) {
        let mut rb = RingBuf::init(cap).unwrap();

        // Fill
        for _ in 0..cap {
            rb.put().unwrap();
            prop_assert!(rb.size_get() <= rb.capacity_get());
            prop_assert!(rb.head_get() < rb.capacity_get());
            prop_assert!(rb.tail_get() < rb.capacity_get());
        }
        prop_assert!(rb.is_full());

        // Drain
        for _ in 0..cap {
            rb.get().unwrap();
            prop_assert!(rb.size_get() <= rb.capacity_get());
            prop_assert!(rb.head_get() < rb.capacity_get());
            prop_assert!(rb.tail_get() < rb.capacity_get());
        }
        prop_assert!(rb.is_empty());
    }

    /// Reset always returns to empty with indices at 0.
    #[test]
    fn prop_reset_always_empties(
        cap in valid_capacity(),
        n_puts in 0..64u32
    ) {
        let mut rb = RingBuf::init(cap).unwrap();
        let n = n_puts.min(cap);
        for _ in 0..n {
            rb.put().unwrap();
        }
        rb.reset();
        prop_assert!(rb.is_empty());
        prop_assert_eq!(rb.head_get(), 0);
        prop_assert_eq!(rb.tail_get(), 0);
    }

    /// Peek returns valid slot indices within bounds.
    #[test]
    fn prop_peek_returns_valid_slots(
        cap in valid_capacity(),
        n_puts in 0..64u32
    ) {
        let mut rb = RingBuf::init(cap).unwrap();
        let n = n_puts.min(cap);

        for _ in 0..n {
            rb.put().unwrap();
        }

        for i in 0..n {
            let slot = rb.peek_at(i).unwrap();
            prop_assert!(slot < cap, "slot {} >= cap {}", slot, cap);
        }
        // Out of bounds
        prop_assert!(rb.peek_at(n).is_err());
    }

    /// Peek does not modify state.
    #[test]
    fn prop_peek_does_not_modify(
        cap in 1..=64u32,
        n in 1..=64u32
    ) {
        let mut rb = RingBuf::init(cap).unwrap();
        let n = n.min(cap);
        for _ in 0..n {
            rb.put().unwrap();
        }
        let before_size = rb.size_get();
        let before_head = rb.head_get();
        let before_tail = rb.tail_get();
        for i in 0..n {
            let _ = rb.peek_at(i);
        }
        prop_assert_eq!(rb.size_get(), before_size);
        prop_assert_eq!(rb.head_get(), before_head);
        prop_assert_eq!(rb.tail_get(), before_tail);
    }

    /// put_n writes min(count, free) bytes.
    #[test]
    fn prop_put_n_writes_correct_amount(
        cap in valid_capacity(),
        count in 0..=2048u32
    ) {
        let mut rb = RingBuf::init(cap).unwrap();
        let free_before = rb.space_get();
        let written = rb.put_n(count);
        prop_assert!(written <= count);
        prop_assert!(written <= free_before);
        prop_assert_eq!(rb.size_get(), written);
    }

    /// get_n reads min(count, size) bytes.
    #[test]
    fn prop_get_n_reads_correct_amount(
        cap in valid_capacity(),
        fill in 0..=1024u32,
        count in 0..=2048u32
    ) {
        let mut rb = RingBuf::init(cap).unwrap();
        let n = fill.min(cap);
        rb.put_n(n);
        let size_before = rb.size_get();
        let read = rb.get_n(count);
        prop_assert!(read <= count);
        prop_assert!(read <= size_before);
        prop_assert_eq!(rb.size_get(), size_before - read);
    }

    /// put_n then get_n roundtrip returns to empty.
    #[test]
    fn prop_put_n_get_n_roundtrip(
        cap in valid_capacity(),
        count in 0..=1024u32
    ) {
        let mut rb = RingBuf::init(cap).unwrap();
        let written = rb.put_n(count);
        let read = rb.get_n(written);
        prop_assert_eq!(read, written);
        prop_assert!(rb.is_empty());
    }

    /// Arbitrary operation sequence: all invariants hold.
    #[test]
    fn prop_arbitrary_ops_maintain_invariant(
        cap in valid_capacity(),
        ops in proptest::collection::vec(
            (0u8..6, 0..256u32),
            1..100
        )
    ) {
        let mut rb = RingBuf::init(cap).unwrap();

        for (op, arg) in ops {
            match op {
                0 => { let _ = rb.put(); }
                1 => { let _ = rb.get(); }
                2 => { rb.reset(); }
                3 => { let _ = rb.peek_at(arg); }
                4 => { rb.put_n(arg); }
                _ => { rb.get_n(arg); }
            }
            // RB1: size bounds
            prop_assert!(rb.size_get() <= rb.capacity_get());
            // RB2: index bounds
            prop_assert!(rb.head_get() < rb.capacity_get());
            prop_assert!(rb.tail_get() < rb.capacity_get());
            // RB7: ring consistency
            let expected_tail =
                (rb.head_get() as u64 + rb.size_get() as u64) % rb.capacity_get() as u64;
            prop_assert_eq!(rb.tail_get() as u64, expected_tail);
            // Conservation
            prop_assert_eq!(rb.size_get() + rb.space_get(), rb.capacity_get());
        }
    }
}
