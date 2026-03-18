//! Property-based tests for the FIFO queue.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::error::*;
use gale::fifo::Fifo;
use proptest::prelude::*;

proptest! {
    /// Init always creates an empty queue.
    #[test]
    fn init_always_empty(_seed in 0u32..10000) {
        let f = Fifo::init();
        prop_assert_eq!(f.num_items(), 0);
        prop_assert!(f.is_empty());
        prop_assert!(!f.peek_head());
    }

    /// Put-get roundtrip preserves state.
    #[test]
    fn put_get_roundtrip(fill in 0u32..=100) {
        let mut f = Fifo::init();
        for _ in 0..fill {
            prop_assert_eq!(f.put(), OK);
        }
        let original = f;

        // Put one more
        prop_assert_eq!(f.put(), OK);
        prop_assert_eq!(f.num_items(), fill + 1);

        // Get brings us back
        prop_assert_eq!(f.get(), OK);
        prop_assert_eq!(f, original);
    }

    /// After arbitrary ops, count is non-negative (trivially true for u32)
    /// and is_empty matches count == 0.
    #[test]
    fn invariant_after_ops(
        ops in proptest::collection::vec(prop_oneof![Just(true), Just(false)], 0..100)
    ) {
        let mut f = Fifo::init();
        for is_put in ops {
            if is_put {
                f.put();
            } else {
                f.get();
            }
            prop_assert_eq!(f.is_empty(), f.num_items() == 0);
            prop_assert_eq!(f.peek_head(), f.num_items() > 0);
        }
    }

    /// Error codes are correct for empty get.
    #[test]
    fn empty_get_returns_eagain(n_puts in 0u32..=50) {
        let mut f = Fifo::init();
        // Put n items
        for _ in 0..n_puts {
            prop_assert_eq!(f.put(), OK);
        }
        // Get n items (drain)
        for _ in 0..n_puts {
            prop_assert_eq!(f.get(), OK);
        }
        // Get on empty -> EAGAIN
        prop_assert_eq!(f.get(), EAGAIN);
    }

    /// Fill-drain symmetric: put N, get N leaves empty.
    #[test]
    fn fill_drain_symmetric(n in 0u32..=100) {
        let mut f = Fifo::init();
        for _ in 0..n {
            f.put();
        }
        prop_assert_eq!(f.num_items(), n);

        for _ in 0..n {
            f.get();
        }
        prop_assert!(f.is_empty());
        prop_assert_eq!(f, Fifo::init());
    }

    /// Count tracks puts minus gets accurately.
    #[test]
    fn count_tracks_net_operations(
        ops in proptest::collection::vec(prop_oneof![Just(true), Just(false)], 0..200)
    ) {
        let mut f = Fifo::init();
        let mut expected_count: u32 = 0;
        for is_put in ops {
            if is_put {
                if f.put() == OK {
                    expected_count += 1;
                }
            } else if f.get() == OK {
                expected_count -= 1;
            }
            prop_assert_eq!(f.num_items(), expected_count);
        }
    }
}
