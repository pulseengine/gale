//! Property-based tests for the memory pool model.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use gale::error::*;
use gale::mempool::MemPool;
use proptest::prelude::*;

proptest! {
    /// MP1: invariant (allocated <= capacity) holds under random ops.
    #[test]
    fn invariant_holds_under_random_ops(
        capacity in 1u32..=500,
        block_size in 1u32..=256,
        ops in proptest::collection::vec(prop_oneof![Just(true), Just(false)], 0..200)
    ) {
        let mut p = MemPool::init(capacity, block_size).unwrap();
        for do_alloc in ops {
            if do_alloc {
                p.alloc();
            } else {
                p.free();
            }
            // MP1: bounds invariant
            prop_assert!(p.allocated_get() <= p.capacity_get());
            // MP5: conservation
            prop_assert_eq!(p.free_get() + p.allocated_get(), capacity);
        }
    }

    /// MP2+MP4: alloc then free roundtrip preserves state.
    #[test]
    fn alloc_free_roundtrip(
        capacity in 1u32..=1000,
        block_size in 1u32..=256,
        fill in 0u32..=999
    ) {
        let fill = fill % capacity; // ensure fill < capacity
        let mut p = MemPool::init(capacity, block_size).unwrap();

        // Fill to `fill` blocks
        for _ in 0..fill {
            p.alloc();
        }
        let original = p;

        // Alloc one more (if room)
        if fill < capacity {
            prop_assert_eq!(p.alloc(), OK);
            prop_assert_eq!(p.allocated_get(), fill + 1);
            // Free brings us back
            prop_assert_eq!(p.free(), OK);
            prop_assert_eq!(p, original);
        }
    }

    /// MP3: full pool rejects alloc.
    #[test]
    fn full_rejects_alloc(capacity in 1u32..=500, block_size in 1u32..=128) {
        let mut p = MemPool::init(capacity, block_size).unwrap();
        for _ in 0..capacity {
            prop_assert_eq!(p.alloc(), OK);
        }
        prop_assert!(p.is_full());
        prop_assert_eq!(p.alloc(), ENOMEM);
    }

    /// MP4: free when empty is rejected.
    #[test]
    fn empty_rejects_free(capacity in 1u32..=500, block_size in 1u32..=128) {
        let mut p = MemPool::init(capacity, block_size).unwrap();
        prop_assert_eq!(p.free(), EINVAL);
    }

    /// MP6: total_size overflow detected.
    #[test]
    fn total_size_overflow_detected(
        capacity in 2u32..=u32::MAX,
        block_size in 2u32..=u32::MAX
    ) {
        let p = MemPool::init(capacity, block_size).unwrap();
        let product = u64::from(capacity) * u64::from(block_size);
        if product > u64::from(u32::MAX) {
            prop_assert!(p.total_size().is_none());
        } else {
            prop_assert_eq!(p.total_size(), Some(product as u32));
        }
    }

    /// alloc_many then free_many roundtrip.
    #[test]
    fn alloc_many_free_many_roundtrip(
        capacity in 1u32..=500,
        block_size in 1u32..=128,
        count in 1u32..=500
    ) {
        let mut p = MemPool::init(capacity, block_size).unwrap();
        if count <= capacity {
            prop_assert_eq!(p.alloc_many(count), OK);
            prop_assert_eq!(p.allocated_get(), count);
            prop_assert_eq!(p.free_many(count), OK);
            prop_assert!(p.is_empty());
        } else {
            prop_assert_eq!(p.alloc_many(count), ENOMEM);
            prop_assert!(p.is_empty());
        }
    }
}
