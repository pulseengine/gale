//! Property-based tests for the event bitmask model.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::event::Event;
use proptest::prelude::*;

proptest! {
    /// Post is monotonic: events never decrease after post.
    #[test]
    fn post_is_monotonic(
        posts in proptest::collection::vec(any::<u32>(), 1..50)
    ) {
        let mut ev = Event::init();
        let mut prev = ev.events_get();

        for bits in posts {
            ev.post(bits);
            let cur = ev.events_get();
            // All bits that were set before are still set
            prop_assert_eq!(prev & cur, prev);
            prev = cur;
        }
    }

    /// Set then clear with the same value yields 0.
    #[test]
    fn set_clear_roundtrip(value in any::<u32>()) {
        let mut ev = Event::init();
        ev.set(value);
        prop_assert_eq!(ev.events_get(), value);

        ev.clear(value);
        prop_assert_eq!(ev.events_get(), 0);
    }

    /// Wait conditions are correct: wait_all implies wait_any for non-zero desired.
    #[test]
    fn wait_conditions_correct(events in any::<u32>(), desired in any::<u32>()) {
        let mut ev = Event::init();
        ev.set(events);

        let any_match = ev.wait_check_any(desired);
        let all_match = ev.wait_check_all(desired);

        // wait_check_any correctness
        prop_assert_eq!(any_match, (events & desired) != 0);
        // wait_check_all correctness
        prop_assert_eq!(all_match, (events & desired) == desired);
        // wait_all implies wait_any (when desired != 0)
        if desired != 0 && all_match {
            prop_assert!(any_match);
        }
    }

    /// set_masked preserves unmasked bits.
    #[test]
    fn set_masked_preserves_unmasked_bits(
        initial in any::<u32>(),
        new_events in any::<u32>(),
        mask in any::<u32>()
    ) {
        let mut ev = Event::init();
        ev.set(initial);

        ev.set_masked(new_events, mask);
        let result = ev.events_get();

        // Bits outside the mask are unchanged
        prop_assert_eq!(result & !mask, initial & !mask);
        // Bits inside the mask come from new_events
        prop_assert_eq!(result & mask, new_events & mask);
    }
}
