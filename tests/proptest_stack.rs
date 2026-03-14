//! Property-based tests for the LIFO stack.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::error::*;
use gale::stack::Stack;
use proptest::prelude::*;

proptest! {
    /// Init with valid capacity always succeeds.
    #[test]
    fn init_valid_params(capacity in 1u32..=10000) {
        let s = Stack::init(capacity).unwrap();
        prop_assert_eq!(s.num_used(), 0);
        prop_assert_eq!(s.num_free(), capacity);
        prop_assert!(s.is_empty());
    }

    /// Push-pop roundtrip on non-full stack preserves state.
    #[test]
    fn push_pop_roundtrip(capacity in 1u32..=100, fill in 0u32..=99) {
        let fill = fill % capacity; // ensure fill < capacity
        let mut s = Stack::init(capacity).unwrap();
        for _ in 0..fill {
            prop_assert_eq!(s.push(), OK);
        }
        let original = s.clone();

        // Push one more (not full since fill < capacity)
        prop_assert_eq!(s.push(), OK);
        prop_assert_eq!(s.num_used(), fill + 1);

        // Pop brings us back
        prop_assert_eq!(s.pop(), OK);
        prop_assert_eq!(s, original);
    }

    /// Conservation: num_free + num_used == capacity after arbitrary ops.
    #[test]
    fn conservation_after_ops(
        capacity in 1u32..=50,
        ops in proptest::collection::vec(prop_oneof![Just(true), Just(false)], 0..100)
    ) {
        let mut s = Stack::init(capacity).unwrap();
        for push in ops {
            if push {
                s.push();
            } else {
                s.pop();
            }
            prop_assert_eq!(s.num_free() + s.num_used(), capacity);
        }
    }

    /// Error codes are correct for full push and empty pop.
    #[test]
    fn error_codes(capacity in 1u32..=50) {
        let mut s = Stack::init(capacity).unwrap();
        // Fill to capacity
        for _ in 0..capacity {
            prop_assert_eq!(s.push(), OK);
        }
        // Push on full -> ENOMEM
        prop_assert_eq!(s.push(), ENOMEM);

        // Drain
        for _ in 0..capacity {
            prop_assert_eq!(s.pop(), OK);
        }
        // Pop on empty -> EBUSY
        prop_assert_eq!(s.pop(), EBUSY);
    }

    /// Fill-drain symmetric: fill N, drain N leaves empty.
    #[test]
    fn fill_drain_symmetric(capacity in 1u32..=100) {
        let mut s = Stack::init(capacity).unwrap();
        for _ in 0..capacity {
            s.push();
        }
        prop_assert!(s.is_full());

        for _ in 0..capacity {
            s.pop();
        }
        prop_assert!(s.is_empty());
        prop_assert_eq!(s, Stack::init(capacity).unwrap());
    }
}
