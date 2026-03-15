//! Property-based tests for the dynamic queue.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::error::*;
use gale::queue::Queue;
use proptest::prelude::*;

proptest! {
    /// Init always creates an empty queue.
    #[test]
    fn init_always_empty(_seed in 0u32..10000) {
        let q = Queue::init();
        prop_assert_eq!(q.count_get(), 0);
        prop_assert!(q.is_empty());
    }

    /// Append-get roundtrip preserves state.
    #[test]
    fn append_get_roundtrip(n in 0u32..100) {
        let mut q = Queue::init();
        for _ in 0..n {
            prop_assert_eq!(q.append(), OK);
        }
        let original = q.clone();

        // Append one more
        prop_assert_eq!(q.append(), OK);
        prop_assert_eq!(q.count_get(), n + 1);

        // Get brings us back
        prop_assert_eq!(q.get(), OK);
        prop_assert_eq!(q, original);
    }

    /// Prepend-get roundtrip preserves state.
    #[test]
    fn prepend_get_roundtrip(n in 0u32..100) {
        let mut q = Queue::init();
        for _ in 0..n {
            prop_assert_eq!(q.append(), OK);
        }
        let original = q.clone();

        // Prepend one more
        prop_assert_eq!(q.prepend(), OK);
        prop_assert_eq!(q.count_get(), n + 1);

        // Get brings us back
        prop_assert_eq!(q.get(), OK);
        prop_assert_eq!(q, original);
    }

    /// Arbitrary operations always maintain count consistency.
    #[test]
    fn arbitrary_ops_consistent(
        ops in proptest::collection::vec(
            prop_oneof![Just(0u8), Just(1u8), Just(2u8)],
            0..200
        )
    ) {
        let mut q = Queue::init();
        let mut expected_count: u32 = 0;

        for op in ops {
            match op {
                0 => {
                    // append
                    let rc = q.append();
                    if expected_count < u32::MAX {
                        prop_assert_eq!(rc, OK);
                        expected_count += 1;
                    } else {
                        prop_assert_eq!(rc, EOVERFLOW);
                    }
                }
                1 => {
                    // prepend
                    let rc = q.prepend();
                    if expected_count < u32::MAX {
                        prop_assert_eq!(rc, OK);
                        expected_count += 1;
                    } else {
                        prop_assert_eq!(rc, EOVERFLOW);
                    }
                }
                _ => {
                    // get
                    let rc = q.get();
                    if expected_count > 0 {
                        prop_assert_eq!(rc, OK);
                        expected_count -= 1;
                    } else {
                        prop_assert_eq!(rc, EAGAIN);
                    }
                }
            }
            prop_assert_eq!(q.count_get(), expected_count);
            prop_assert_eq!(q.is_empty(), expected_count == 0);
        }
    }

    /// Fill-drain symmetric: N appends followed by N gets leaves empty.
    #[test]
    fn fill_drain_symmetric(n in 1u32..=200) {
        let mut q = Queue::init();
        for _ in 0..n {
            q.append();
        }
        prop_assert_eq!(q.count_get(), n);

        for _ in 0..n {
            q.get();
        }
        prop_assert!(q.is_empty());
        prop_assert_eq!(q, Queue::init());
    }

    /// Error codes are correct for empty get.
    #[test]
    fn empty_get_returns_eagain(_seed in 0u32..100) {
        let mut q = Queue::init();
        prop_assert_eq!(q.get(), EAGAIN);
    }

    /// Mixed append/prepend all increment count equally.
    #[test]
    fn append_prepend_equivalent(
        ops in proptest::collection::vec(proptest::bool::ANY, 1..100)
    ) {
        let mut q = Queue::init();
        let mut count: u32 = 0;
        for use_append in &ops {
            if *use_append {
                prop_assert_eq!(q.append(), OK);
            } else {
                prop_assert_eq!(q.prepend(), OK);
            }
            count += 1;
            prop_assert_eq!(q.count_get(), count);
        }
    }
}
