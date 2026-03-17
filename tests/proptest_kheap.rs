//! Property-based tests for the kernel heap model.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::error::*;
use gale::kheap::KHeap;
use proptest::prelude::*;

proptest! {
    /// KH1: invariant (allocated_bytes <= capacity) holds under random ops.
    #[test]
    fn invariant_holds_under_random_ops(
        capacity in 1u32..=10000,
        ops in proptest::collection::vec((prop_oneof![Just(true), Just(false)], 1u32..=200), 0..100)
    ) {
        let mut h = KHeap::init(capacity).unwrap();
        for (do_alloc, bytes) in ops {
            if do_alloc {
                h.alloc(bytes);
            } else {
                h.free(bytes);
            }
            // KH1: bounds invariant
            prop_assert!(h.allocated_get() <= h.capacity_get());
            // KH5: conservation
            prop_assert_eq!(h.free_get() + h.allocated_get(), capacity);
        }
    }

    /// KH2+KH4: alloc(n) then free(n) roundtrip preserves state.
    #[test]
    fn alloc_free_roundtrip(
        capacity in 1u32..=10000,
        fill in 0u32..=9999,
        alloc_size in 1u32..=5000
    ) {
        let fill = fill % capacity; // ensure fill < capacity
        let mut h = KHeap::init(capacity).unwrap();

        // Fill to `fill` bytes
        if fill > 0 {
            prop_assert_eq!(h.alloc(fill), OK);
        }
        let original = h;

        // Try to alloc alloc_size more
        let remaining = capacity - fill;
        if alloc_size <= remaining {
            prop_assert_eq!(h.alloc(alloc_size), OK);
            prop_assert_eq!(h.allocated_get(), fill + alloc_size);

            // Free brings us back
            prop_assert_eq!(h.free(alloc_size), OK);
            prop_assert_eq!(h, original);
        } else {
            prop_assert_eq!(h.alloc(alloc_size), ENOMEM);
            prop_assert_eq!(h, original);
        }
    }

    /// KH5: conservation (free + allocated == capacity) after arbitrary ops.
    #[test]
    fn conservation_after_ops(
        capacity in 1u32..=5000,
        ops in proptest::collection::vec((prop_oneof![Just(true), Just(false)], 1u32..=100), 0..100)
    ) {
        let mut h = KHeap::init(capacity).unwrap();
        for (do_alloc, bytes) in ops {
            if do_alloc {
                h.alloc(bytes);
            } else {
                h.free(bytes);
            }
            prop_assert_eq!(h.free_get() + h.allocated_get(), capacity);
        }
    }

    /// KH3: full heap rejects alloc.
    #[test]
    fn full_rejects_alloc(capacity in 1u32..=5000) {
        let mut h = KHeap::init(capacity).unwrap();
        prop_assert_eq!(h.alloc(capacity), OK);
        prop_assert!(h.is_full());
        prop_assert_eq!(h.alloc(1), ENOMEM);
    }

    /// KH4: free more than allocated is rejected.
    #[test]
    fn free_more_than_allocated_rejected(
        capacity in 1u32..=5000,
        alloc_size in 1u32..=4999
    ) {
        let alloc_size = alloc_size % capacity + 1; // ensure 1 <= alloc_size <= capacity
        let mut h = KHeap::init(capacity).unwrap();
        prop_assert_eq!(h.alloc(alloc_size.min(capacity)), OK);

        let allocated = h.allocated_get();
        if allocated < capacity {
            prop_assert_eq!(h.free(allocated + 1), EINVAL);
            prop_assert_eq!(h.allocated_get(), allocated);
        }
    }

    /// KH6: calloc multiplication overflow detected.
    #[test]
    fn calloc_overflow_detected(
        capacity in 1u32..=u32::MAX,
        num in 2u32..=u32::MAX,
        size in 2u32..=u32::MAX
    ) {
        let mut h = KHeap::init(capacity).unwrap();
        let product = u64::from(num) * u64::from(size);
        if product > u64::from(u32::MAX) {
            prop_assert_eq!(h.calloc(num, size), ENOMEM);
            prop_assert_eq!(h.allocated_get(), 0);
        }
    }

    /// Aligned alloc has same accounting as regular alloc.
    #[test]
    fn aligned_alloc_equivalent(
        capacity in 1u32..=5000,
        bytes in 1u32..=5000
    ) {
        let mut h1 = KHeap::init(capacity).unwrap();
        let mut h2 = KHeap::init(capacity).unwrap();

        let rc1 = h1.alloc(bytes);
        let rc2 = h2.aligned_alloc(bytes, 0);

        prop_assert_eq!(rc1, rc2);
        prop_assert_eq!(h1.allocated_get(), h2.allocated_get());
    }
}
