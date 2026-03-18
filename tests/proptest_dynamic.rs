//! Property-based tests for the dynamic thread pool model.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::dynamic::DynamicPool;
use gale::error::*;
use proptest::prelude::*;

proptest! {
    /// DY1: invariant (active <= max_threads) holds under random ops.
    #[test]
    fn invariant_holds_under_random_ops(
        max_threads in 1u32..=100,
        stack_size in 1u32..=8192,
        ops in proptest::collection::vec(prop_oneof![Just(true), Just(false)], 0..200)
    ) {
        let mut p = DynamicPool::init(max_threads, stack_size).unwrap();
        for do_alloc in ops {
            if do_alloc {
                p.alloc();
            } else {
                p.free();
            }
            // DY1: bounds invariant
            prop_assert!(p.active_get() <= p.max_threads_get());
            // Conservation
            prop_assert_eq!(p.active_get() + p.available_get(), max_threads);
        }
    }

    /// DY2+DY4: alloc then free roundtrip preserves state.
    #[test]
    fn alloc_free_roundtrip(
        max_threads in 1u32..=100,
        stack_size in 1u32..=8192,
        fill in 0u32..=99
    ) {
        let fill = fill % max_threads; // ensure fill < max_threads
        let mut p = DynamicPool::init(max_threads, stack_size).unwrap();

        for _ in 0..fill {
            p.alloc();
        }
        let original = p;

        if fill < max_threads {
            prop_assert_eq!(p.alloc(), OK);
            prop_assert_eq!(p.active_get(), fill + 1);
            prop_assert_eq!(p.free(), OK);
            prop_assert_eq!(p, original);
        }
    }

    /// DY3: full pool rejects alloc.
    #[test]
    fn full_rejects_alloc(max_threads in 1u32..=100, stack_size in 1u32..=4096) {
        let mut p = DynamicPool::init(max_threads, stack_size).unwrap();
        for _ in 0..max_threads {
            prop_assert_eq!(p.alloc(), OK);
        }
        prop_assert!(p.is_full());
        prop_assert_eq!(p.alloc(), ENOMEM);
    }

    /// DY4: free when empty is rejected.
    #[test]
    fn empty_rejects_free(max_threads in 1u32..=100, stack_size in 1u32..=4096) {
        let mut p = DynamicPool::init(max_threads, stack_size).unwrap();
        prop_assert_eq!(p.free(), EINVAL);
    }

    /// can_serve is correct for arbitrary sizes.
    #[test]
    fn can_serve_correct(
        max_threads in 1u32..=100,
        stack_size in 1u32..=8192,
        requested in 0u32..=16384
    ) {
        let p = DynamicPool::init(max_threads, stack_size).unwrap();
        prop_assert_eq!(p.can_serve(requested), requested <= stack_size);
    }

    /// Stress: alloc to full, free to empty.
    #[test]
    fn stress_alloc_free(max_threads in 1u32..=50, stack_size in 1u32..=2048) {
        let mut p = DynamicPool::init(max_threads, stack_size).unwrap();
        for _ in 0..max_threads {
            prop_assert_eq!(p.alloc(), OK);
        }
        prop_assert!(p.is_full());
        for _ in 0..max_threads {
            prop_assert_eq!(p.free(), OK);
        }
        prop_assert!(p.is_empty());
    }
}
