//! Property-based tests for the LIFO queue.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::error::*;
use gale::lifo::Lifo;
use proptest::prelude::*;

proptest! {
    /// Put-get roundtrip preserves state.
    #[test]
    fn put_get_roundtrip(n in 0u32..=100) {
        let mut q = Lifo::init();
        for _ in 0..n {
            prop_assert_eq!(q.put(), OK);
        }
        let original = q;

        // Put one more
        prop_assert_eq!(q.put(), OK);
        prop_assert_eq!(q.num_items(), n + 1);

        // Get brings us back
        prop_assert_eq!(q.get(), OK);
        prop_assert_eq!(q, original);
    }

    /// Empty queue always returns EAGAIN.
    #[test]
    fn empty_get_returns_eagain(puts in 0u32..=50) {
        let mut q = Lifo::init();
        // Put then drain
        for _ in 0..puts {
            q.put();
        }
        for _ in 0..puts {
            q.get();
        }
        prop_assert!(q.is_empty());
        prop_assert_eq!(q.get(), EAGAIN);
    }

    /// Count tracks puts minus gets.
    #[test]
    fn count_tracks_ops(
        ops in proptest::collection::vec(prop_oneof![Just(true), Just(false)], 0..200)
    ) {
        let mut q = Lifo::init();
        let mut expected: u32 = 0;
        for is_put in ops {
            if is_put {
                q.put();
                expected += 1;
            } else if expected > 0 {
                prop_assert_eq!(q.get(), OK);
                expected -= 1;
            } else {
                prop_assert_eq!(q.get(), EAGAIN);
            }
            prop_assert_eq!(q.num_items(), expected);
        }
    }

    /// Fill-drain symmetric: put N, get N leaves empty.
    #[test]
    fn fill_drain_symmetric(n in 1u32..=200) {
        let mut q = Lifo::init();
        for _ in 0..n {
            q.put();
        }
        prop_assert_eq!(q.num_items(), n);

        for _ in 0..n {
            q.get();
        }
        prop_assert!(q.is_empty());
        prop_assert_eq!(q, Lifo::init());
    }

    /// is_empty matches count == 0.
    #[test]
    fn is_empty_matches_count(n in 0u32..=100) {
        let mut q = Lifo::init();
        for _ in 0..n {
            q.put();
        }
        prop_assert_eq!(q.is_empty(), q.num_items() == 0);
    }
}
