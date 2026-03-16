//! Property-based tests for the timeout model.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::error::*;
use gale::timeout::{K_FOREVER_TICKS, K_NO_WAIT_TICKS, Timeout};
use proptest::prelude::*;

/// Strategy for valid current_tick values (must be < u64::MAX).
fn valid_tick() -> impl Strategy<Value = u64> {
    0u64..=(u64::MAX / 2)
}

/// Strategy for valid durations that won't overflow when added to a tick.
fn valid_duration(max_tick: u64) -> impl Strategy<Value = u64> {
    let max_dur = if max_tick >= K_FOREVER_TICKS - 1 {
        0u64
    } else {
        K_FOREVER_TICKS - 1 - max_tick
    };
    0u64..=max_dur
}

proptest! {
    /// TO1: after add, deadline >= current_tick.
    #[test]
    fn deadline_ge_current_tick(
        tick in valid_tick(),
        duration in 0u64..=(u64::MAX / 4)
    ) {
        let mut t = Timeout::init(tick);
        if let Ok(deadline) = t.add(duration) {
            prop_assert!(deadline >= tick);
            prop_assert_eq!(deadline, tick + duration);
        }
    }

    /// TO2: add sets deadline = now + duration.
    #[test]
    fn add_sets_correct_deadline(
        tick in valid_tick(),
        duration in 0u64..=(u64::MAX / 4)
    ) {
        let mut t = Timeout::init(tick);
        if let Ok(deadline) = t.add(duration) {
            prop_assert_eq!(deadline, tick + duration);
            prop_assert!(t.is_active());
        }
    }

    /// TO3: abort always deactivates.
    #[test]
    fn abort_deactivates(
        tick in valid_tick(),
        duration in 0u64..=(u64::MAX / 4)
    ) {
        let mut t = Timeout::init(tick);
        if t.add(duration).is_ok() {
            prop_assert!(t.is_active());
            let rc = t.abort();
            prop_assert_eq!(rc, OK);
            prop_assert!(!t.is_active());
        }
    }

    /// TO4: announce fires when deadline <= new tick.
    #[test]
    fn announce_fires_expired(
        tick in 0u64..=(u64::MAX / 4),
        duration in 1u64..=1000,
        extra in 0u64..=1000
    ) {
        let mut t = Timeout::init(tick);
        if t.add(duration).is_ok() {
            // Advance past deadline
            let advance = duration + extra;
            if let Ok(fired) = t.announce(advance) {
                prop_assert!(fired);
                prop_assert!(!t.is_active());
            }
        }
    }

    /// TO4: announce does NOT fire when deadline > new tick.
    #[test]
    fn announce_does_not_fire_early(
        tick in 0u64..=(u64::MAX / 4),
        duration in 2u64..=10000,
    ) {
        let mut t = Timeout::init(tick);
        if t.add(duration).is_ok() {
            // Advance less than duration
            let partial = duration - 1;
            if let Ok(fired) = t.announce(partial) {
                prop_assert!(!fired);
                prop_assert!(t.is_active());
                prop_assert_eq!(t.remaining(), 1);
            }
        }
    }

    /// TO5: overflow is rejected.
    #[test]
    fn overflow_rejected(tick in 1u64..=(u64::MAX / 2)) {
        let mut t = Timeout::init(tick);
        // Duration that would overflow to u64::MAX
        let big_duration = K_FOREVER_TICKS - tick;
        prop_assert_eq!(t.add(big_duration), Err(EINVAL));
        prop_assert!(!t.is_active());
    }

    /// TO6: timepoint_calc roundtrip.
    #[test]
    fn timepoint_roundtrip(
        tick in valid_tick(),
        duration in 1u64..=(u64::MAX / 4)
    ) {
        let t = Timeout::init(tick);
        if let Ok(tp) = t.timepoint_calc(duration) {
            let back = t.timepoint_timeout(tp);
            prop_assert_eq!(back, duration);
        }
    }

    /// TO7: forever timeout never fires.
    #[test]
    fn forever_never_fires(
        tick in 0u64..=(u64::MAX / 4),
        advance in 1u64..=(u64::MAX / 4)
    ) {
        let mut t = Timeout::init(tick);
        let forever = t.add_forever();
        let mut t2 = forever;
        if let Ok(fired) = t2.announce(advance) {
            prop_assert!(!fired);
            prop_assert!(t2.is_active());
        }
    }

    /// TO8: no-wait timeout fires on any positive advance.
    #[test]
    fn no_wait_fires_immediately(
        tick in 0u64..=(u64::MAX / 4),
        advance in 0u64..=1000
    ) {
        let mut t = Timeout::init(tick);
        let immediate = t.add_no_wait();
        let mut t2 = immediate;
        if let Ok(fired) = t2.announce(advance) {
            // deadline 0 is always <= current_tick (which is >= tick >= 0)
            prop_assert!(fired);
            prop_assert!(!t2.is_active());
        }
    }

    /// Remaining decreases as ticks advance.
    #[test]
    fn remaining_decreases(
        tick in 0u64..=(u64::MAX / 8),
        duration in 10u64..=10000,
        partial in 1u64..=9
    ) {
        let mut t = Timeout::init(tick);
        if t.add(duration).is_ok() {
            let r1 = t.remaining();
            prop_assert_eq!(r1, duration);

            if t.announce(partial).is_ok() && t.is_active() {
                let r2 = t.remaining();
                prop_assert!(r2 < r1);
                prop_assert_eq!(r2, duration - partial);
            }
        }
    }

    /// Abort then re-add works correctly.
    #[test]
    fn abort_readd(
        tick in 0u64..=(u64::MAX / 4),
        dur1 in 1u64..=1000,
        dur2 in 1u64..=1000
    ) {
        let mut t = Timeout::init(tick);
        if t.add(dur1).is_ok() {
            t.abort();
            prop_assert!(!t.is_active());

            if let Ok(deadline) = t.add(dur2) {
                prop_assert_eq!(deadline, tick + dur2);
                prop_assert!(t.is_active());
            }
        }
    }

    /// Random operation sequence maintains invariant.
    #[test]
    fn random_ops_maintain_invariant(
        tick in 0u64..=10000,
        ops in proptest::collection::vec(0u8..5, 0..50)
    ) {
        let mut t = Timeout::init(tick);
        for op in ops {
            match op {
                0 => {
                    // Try to add (only if inactive)
                    if !t.is_active() {
                        let _ = t.add(100);
                    }
                }
                1 => {
                    let _ = t.abort();
                }
                2 => {
                    let _ = t.announce(10);
                }
                3 => {
                    let _ = t.remaining();
                }
                _ => {
                    let _ = t.expires();
                }
            }
            // current_tick must always be < K_FOREVER_TICKS
            prop_assert!(t.current_tick < K_FOREVER_TICKS);
            // If active, deadline >= current_tick (unless deadline is 0/no-wait)
            if t.is_active() && t.deadline != K_NO_WAIT_TICKS {
                prop_assert!(t.deadline >= t.current_tick);
            }
        }
    }

    /// add_absolute with valid deadline succeeds.
    #[test]
    fn add_absolute_valid(
        tick in 0u64..=(u64::MAX / 4),
        offset in 0u64..=10000
    ) {
        let mut t = Timeout::init(tick);
        let deadline = tick + offset;
        if deadline < K_FOREVER_TICKS {
            let result = t.add_absolute(deadline);
            prop_assert!(result.is_ok());
            prop_assert_eq!(result.unwrap(), deadline);
            prop_assert!(t.is_active());
        }
    }
}
