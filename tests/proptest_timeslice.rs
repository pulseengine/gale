//! Property-based tests for the time-slicing model.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::timeslice::TimeSlice;
use proptest::prelude::*;

proptest! {
    /// TS1: invariant (slice_ticks <= slice_max_ticks) holds under random ops.
    #[test]
    fn invariant_holds_under_random_ops(
        max_ticks in 1u32..=1000,
        ops in proptest::collection::vec(0u8..4, 0..200)
    ) {
        let mut ts = TimeSlice::init_disabled();
        ts.set_config(max_ticks);

        for op in ops {
            match op {
                0 => ts.tick(),
                1 => ts.reset(),
                2 => { ts.consume_expired(); }
                _ => ts.set_config(max_ticks),
            }
            // TS1: bounds invariant
            prop_assert!(ts.remaining() <= ts.max_ticks());
        }
    }

    /// TS2: reset always restores slice_ticks to slice_max_ticks.
    #[test]
    fn reset_restores_max(
        max_ticks in 1u32..=1000,
        n_ticks in 0u32..=999
    ) {
        let n_ticks = n_ticks % (max_ticks + 1);
        let mut ts = TimeSlice::init_disabled();
        ts.set_config(max_ticks);

        for _ in 0..n_ticks {
            ts.tick();
        }

        ts.reset();
        prop_assert_eq!(ts.remaining(), max_ticks);
        prop_assert!(!ts.is_expired());
    }

    /// TS3+TS4: N ticks from max counts down to max-N, expires at 0.
    #[test]
    fn countdown_correct(max_ticks in 1u32..=500) {
        let mut ts = TimeSlice::init_disabled();
        ts.set_config(max_ticks);

        for i in 1..=max_ticks {
            prop_assert!(!ts.is_expired());
            ts.tick();
            prop_assert_eq!(ts.remaining(), max_ticks - i);
        }
        prop_assert!(ts.is_expired());
        prop_assert_eq!(ts.remaining(), 0);
    }

    /// TS5: tick at 0 does not underflow — remaining stays 0.
    #[test]
    fn no_underflow(max_ticks in 1u32..=100, extra_ticks in 1u32..=50) {
        let mut ts = TimeSlice::init_disabled();
        ts.set_config(max_ticks);

        // Count down to 0
        for _ in 0..max_ticks {
            ts.tick();
        }
        prop_assert_eq!(ts.remaining(), 0);

        // Extra ticks at 0 — no underflow
        for _ in 0..extra_ticks {
            ts.tick();
            prop_assert_eq!(ts.remaining(), 0);
            prop_assert!(ts.is_expired());
        }
    }

    /// TS6: enabled implies max_ticks > 0.
    #[test]
    fn enabled_implies_positive_max(max_ticks in 0u32..=1000) {
        let mut ts = TimeSlice::init_disabled();
        ts.set_config(max_ticks);

        if ts.is_enabled() {
            prop_assert!(ts.max_ticks() > 0);
        } else {
            prop_assert_eq!(ts.max_ticks(), 0);
        }
    }

    /// Reconfiguration preserves invariant.
    #[test]
    fn reconfig_preserves_invariant(
        max1 in 1u32..=500,
        max2 in 0u32..=500,
        ticks_between in 0u32..=100
    ) {
        let mut ts = TimeSlice::init_disabled();
        ts.set_config(max1);

        let ticks = ticks_between % (max1 + 1);
        for _ in 0..ticks {
            ts.tick();
        }

        ts.set_config(max2);
        prop_assert!(ts.remaining() <= ts.max_ticks());
        prop_assert!(!ts.is_expired());
    }

    /// consume_expired is idempotent (second call returns false).
    #[test]
    fn consume_expired_idempotent(max_ticks in 1u32..=100) {
        let mut ts = TimeSlice::init_disabled();
        ts.set_config(max_ticks);

        // Count down to expiry
        for _ in 0..max_ticks {
            ts.tick();
        }
        prop_assert!(ts.is_expired());

        let first = ts.consume_expired();
        prop_assert!(first);
        prop_assert!(!ts.is_expired());

        let second = ts.consume_expired();
        prop_assert!(!second);
    }
}
