//! Property-based tests for the memory slab allocator.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::error::*;
use gale::mem_slab::MemSlab;
use proptest::prelude::*;

proptest! {
    /// Invariant holds after random alloc/free sequences.
    #[test]
    fn invariant_holds_under_random_ops(
        num_blocks in 1u32..=50,
        block_size in 1u32..=256,
        ops in proptest::collection::vec(prop_oneof![Just(true), Just(false)], 0..100)
    ) {
        let mut s = MemSlab::init(block_size, num_blocks).unwrap();
        for do_alloc in ops {
            if do_alloc {
                s.alloc();
            } else {
                s.free();
            }
            // MS1: bounds invariant
            prop_assert!(s.num_used_get() <= s.num_blocks_get());
            // MS7: conservation
            prop_assert_eq!(s.num_free_get() + s.num_used_get(), num_blocks);
        }
    }

    /// Alloc-free roundtrip preserves state.
    #[test]
    fn alloc_free_roundtrip(
        num_blocks in 1u32..=100,
        block_size in 1u32..=256,
        fill in 0u32..=99
    ) {
        let fill = fill % num_blocks; // ensure fill < num_blocks
        let mut s = MemSlab::init(block_size, num_blocks).unwrap();
        for _ in 0..fill {
            prop_assert_eq!(s.alloc(), OK);
        }
        let original = s.clone();

        // Alloc one more (not full since fill < num_blocks)
        prop_assert_eq!(s.alloc(), OK);
        prop_assert_eq!(s.num_used_get(), fill + 1);

        // Free brings us back
        prop_assert_eq!(s.free(), OK);
        prop_assert_eq!(s, original);
    }

    /// Conservation: num_free + num_used == num_blocks after arbitrary ops.
    #[test]
    fn conservation_after_ops(
        num_blocks in 1u32..=50,
        block_size in 1u32..=256,
        ops in proptest::collection::vec(prop_oneof![Just(true), Just(false)], 0..100)
    ) {
        let mut s = MemSlab::init(block_size, num_blocks).unwrap();
        for do_alloc in ops {
            if do_alloc {
                s.alloc();
            } else {
                s.free();
            }
            prop_assert_eq!(s.num_free_get() + s.num_used_get(), num_blocks);
        }
    }

    /// Error codes: full -> ENOMEM, all-free -> EINVAL.
    #[test]
    fn error_codes(num_blocks in 1u32..=50, block_size in 1u32..=256) {
        let mut s = MemSlab::init(block_size, num_blocks).unwrap();

        // Fill to capacity
        for _ in 0..num_blocks {
            prop_assert_eq!(s.alloc(), OK);
        }
        // Alloc on full -> ENOMEM
        prop_assert_eq!(s.alloc(), ENOMEM);

        // Drain
        for _ in 0..num_blocks {
            prop_assert_eq!(s.free(), OK);
        }
        // Free when all free -> EINVAL
        prop_assert_eq!(s.free(), EINVAL);
    }
}
