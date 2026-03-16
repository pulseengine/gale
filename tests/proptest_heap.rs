//! Property-based tests for the sys_heap chunk allocator model.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::error::*;
use gale::heap::{CHUNK_UNIT, Heap};
use proptest::prelude::*;

proptest! {
    /// HP1: allocated_bytes <= capacity after random ops.
    #[test]
    fn hp1_bounds_invariant(
        capacity in 100u32..=10000,
        overhead in 10u32..=99,
        ops in proptest::collection::vec(
            (prop_oneof![Just(0u8), Just(1), Just(2), Just(3)], 1u32..=200),
            0..80
        )
    ) {
        let overhead = overhead.min(capacity - 1);
        let mut h = Heap::init(capacity, overhead).unwrap();

        for (op, val) in ops {
            match op {
                0 => { let _ = h.alloc(val); }
                1 => { h.free(val); }
                2 => { h.split(val, val.saturating_add(1)); }
                3 => { h.merge(); }
                _ => {}
            }
            // HP1: bounds
            prop_assert!(h.allocated_get() <= h.capacity_get(),
                "HP1 violated: allocated {} > capacity {}", h.allocated_get(), h.capacity_get());
            // HP1: byte conservation
            prop_assert_eq!(h.free_bytes_get() + h.allocated_get(), capacity);
        }
    }

    /// HP2: free_chunks + used_chunks == total_chunks after random ops.
    #[test]
    fn hp2_chunk_conservation(
        capacity in 100u32..=5000,
        overhead in 10u32..=99,
        ops in proptest::collection::vec(
            (prop_oneof![Just(0u8), Just(1), Just(2), Just(3)], 1u32..=100),
            0..80
        )
    ) {
        let overhead = overhead.min(capacity - 1);
        let mut h = Heap::init(capacity, overhead).unwrap();

        for (op, val) in ops {
            match op {
                0 => { let _ = h.alloc(val); }
                1 => { h.free(val); }
                2 => { h.split(val, val.saturating_add(1)); }
                3 => { h.merge(); }
                _ => {}
            }
            prop_assert_eq!(
                h.used_chunks_get() + h.free_chunks_get(),
                h.total_chunks_get(),
                "HP2 violated"
            );
        }
    }

    /// HP3+HP4: alloc(n) then free(n) roundtrip preserves state.
    #[test]
    fn hp3_hp4_alloc_free_roundtrip(
        capacity in 200u32..=10000,
        overhead in 10u32..=99,
        alloc_size in 1u32..=5000
    ) {
        let overhead = overhead.min(capacity - 1);
        let mut h = Heap::init(capacity, overhead).unwrap();
        let original_alloc = h.allocated_get();
        let original_free_chunks = h.free_chunks_get();

        let remaining = capacity - overhead;
        if alloc_size <= remaining {
            let slot = h.alloc(alloc_size).unwrap();
            prop_assert!(slot > 0);
            prop_assert_eq!(h.allocated_get(), original_alloc + alloc_size);

            prop_assert_eq!(h.free(alloc_size), OK);
            prop_assert_eq!(h.allocated_get(), original_alloc);
            prop_assert_eq!(h.free_chunks_get(), original_free_chunks);
        } else {
            prop_assert!(h.alloc(alloc_size).is_err());
            prop_assert_eq!(h.allocated_get(), original_alloc);
        }
    }

    /// HP5: double-free always rejected.
    /// When free_chunks == total_chunks, no used chunks remain and
    /// further free must be rejected.
    #[test]
    fn hp5_double_free_rejected(
        capacity in 200u32..=5000,
        overhead in 10u32..=99
    ) {
        let overhead = overhead.min(capacity - 1);
        let mut h = Heap::init(capacity, overhead).unwrap();

        let remaining = capacity - overhead;
        if remaining >= 10 {
            h.alloc(10).unwrap();
            // free_chunks=0, total_chunks=2
            prop_assert_eq!(h.free(10), OK);
            // free_chunks=1, total_chunks=2, allocated=overhead
            // Free the overhead bytes too to reach free_chunks == total_chunks
            prop_assert_eq!(h.free(overhead), OK);
            // free_chunks=2 == total_chunks=2 => double-free
            prop_assert_eq!(h.free(1), EINVAL);
        }
    }

    /// HP6: aligned_alloc with align <= CHUNK_UNIT behaves like plain alloc.
    #[test]
    fn hp6_small_align_equivalence(
        capacity in 200u32..=5000,
        overhead in 10u32..=99,
        bytes in 1u32..=100,
        align in 0u32..=8
    ) {
        let overhead = overhead.min(capacity - 1);
        let mut h1 = Heap::init(capacity, overhead).unwrap();
        let mut h2 = Heap::init(capacity, overhead).unwrap();

        let r1 = h1.alloc(bytes);
        let r2 = h2.aligned_alloc(bytes, align);

        match (r1, r2) {
            (Ok(s1), Ok(s2)) => {
                prop_assert_eq!(s1, s2);
                prop_assert_eq!(h1.allocated_get(), h2.allocated_get());
            }
            (Err(_), Err(_)) => {} // both failed, ok
            _ => prop_assert!(false, "alloc and aligned_alloc disagree for small align"),
        }
    }

    /// HP7: bytes_to_chunks never overflows.
    #[test]
    fn hp7_bytes_to_chunks_safe(bytes in 0u32..=u32::MAX) {
        // Must not panic
        let chunks = Heap::bytes_to_chunks(bytes);
        // Result should be ceiling division
        let expected = ((bytes as u64) + (CHUNK_UNIT as u64) - 1) / (CHUNK_UNIT as u64);
        prop_assert_eq!(chunks as u64, expected);
    }

    /// HP8: split then merge is identity on chunk counts.
    #[test]
    fn hp8_split_merge_identity(
        capacity in 200u32..=5000,
        overhead in 10u32..=99
    ) {
        let overhead = overhead.min(capacity - 1);
        let mut h = Heap::init(capacity, overhead).unwrap();
        let orig_total = h.total_chunks_get();
        let orig_free = h.free_chunks_get();
        let orig_alloc = h.allocated_get();

        // Split
        prop_assert_eq!(h.split(10, 20), OK);
        prop_assert_eq!(h.total_chunks_get(), orig_total + 1);
        prop_assert_eq!(h.free_chunks_get(), orig_free + 1);

        // Merge reverses
        prop_assert_eq!(h.merge(), OK);
        prop_assert_eq!(h.total_chunks_get(), orig_total);
        prop_assert_eq!(h.free_chunks_get(), orig_free);
        prop_assert_eq!(h.allocated_get(), orig_alloc);
    }

    /// HP3: full heap rejects alloc.
    #[test]
    fn hp3_full_rejects_alloc(
        capacity in 100u32..=5000,
        overhead in 10u32..=99
    ) {
        let overhead = overhead.min(capacity - 1);
        let mut h = Heap::init(capacity, overhead).unwrap();
        let remaining = capacity - overhead;
        // Fill to capacity
        prop_assert!(h.alloc(remaining).is_ok());
        prop_assert!(h.is_full());
        // Any further alloc rejected
        prop_assert!(h.alloc(1).is_err());
    }

    /// Realloc shrink always succeeds.
    #[test]
    fn realloc_shrink_succeeds(
        capacity in 200u32..=5000,
        overhead in 10u32..=99,
        alloc_size in 10u32..=4000,
        new_size in 1u32..=10
    ) {
        let overhead = overhead.min(capacity - 1);
        let mut h = Heap::init(capacity, overhead).unwrap();

        let remaining = capacity - overhead;
        let alloc_size = alloc_size.min(remaining);
        if alloc_size > 0 {
            h.alloc(alloc_size).unwrap();
            let new_size = new_size.min(alloc_size);
            let result = h.realloc(alloc_size, new_size);
            prop_assert!(result.is_ok());
            let expected = overhead + new_size;
            prop_assert_eq!(h.allocated_get(), expected);
        }
    }

    /// Coalesce free preserves byte conservation.
    #[test]
    fn coalesce_preserves_conservation(
        capacity in 200u32..=5000,
        overhead in 10u32..=99,
        alloc_size in 1u32..=100
    ) {
        let overhead = overhead.min(capacity - 1);
        let mut h = Heap::init(capacity, overhead).unwrap();
        let remaining = capacity - overhead;
        let alloc_size = alloc_size.min(remaining);
        if alloc_size > 0 {
            h.alloc(alloc_size).unwrap();
            // Now free_chunks==0, so coalesce should work
            let rc = h.coalesce_free(alloc_size, false, false);
            prop_assert_eq!(rc, OK);
            prop_assert_eq!(h.free_bytes_get() + h.allocated_get(), capacity);
        }
    }
}
