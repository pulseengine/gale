//! Property-based tests for the timer model.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::error::*;
use gale::timer::Timer;
use proptest::prelude::*;

proptest! {
    /// Invariant holds after random start/stop/expire/status_get sequences.
    #[test]
    fn invariant_holds_under_random_ops(
        period in 0u32..=1000,
        ops in proptest::collection::vec(0u8..4, 0..100)
    ) {
        let mut t = Timer::init(period);
        for op in ops {
            match op {
                0 => t.start(),
                1 => t.stop(),
                2 => { let _ = t.expire(); }
                _ => { t.status_get(); }
            }
            // Period never changes
            prop_assert_eq!(t.period_get(), period);
        }
    }

    /// Expire then status_get returns the accumulated count and resets to 0.
    #[test]
    fn expire_status_get_roundtrip(
        period in 0u32..=1000,
        n in 1u32..=100
    ) {
        let mut t = Timer::init(period);
        t.start();

        for _ in 0..n {
            prop_assert!(t.expire().is_ok());
        }
        prop_assert_eq!(t.status_peek(), n);

        let got = t.status_get();
        prop_assert_eq!(got, n);
        prop_assert_eq!(t.status_peek(), 0);
    }

    /// Start always resets status to 0 regardless of prior state.
    #[test]
    fn start_resets_status(
        period in 0u32..=1000,
        n in 0u32..=50
    ) {
        let mut t = Timer::init(period);
        t.start();
        for _ in 0..n {
            let _ = t.expire();
        }
        prop_assert_eq!(t.status_peek(), n);

        // Restart resets status
        t.start();
        prop_assert_eq!(t.status_peek(), 0);
        prop_assert!(t.is_running());
    }

    /// Expire at u32::MAX returns EOVERFLOW and leaves status unchanged.
    #[test]
    fn overflow_rejected(period in 0u32..=1000) {
        let mut t = Timer::init(period);
        t.start();

        // Force status to MAX via direct field access
        t.status = u32::MAX;
        let result = t.expire();
        prop_assert!(result.is_err());
        prop_assert_eq!(result.unwrap_err(), EOVERFLOW);
        prop_assert_eq!(t.status_peek(), u32::MAX);
    }
}
