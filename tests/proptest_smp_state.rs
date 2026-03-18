//! Property-based tests for the SMP state tracking model.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::error::*;
use gale::smp_state::*;
use proptest::prelude::*;

proptest! {
    /// SM1: invariant (1 <= active <= max_cpus) holds under random ops.
    #[test]
    fn invariant_holds_under_random_ops(
        max_cpus in 1u32..=MAX_CPUS,
        ops in proptest::collection::vec(
            prop_oneof![Just(0u8), Just(1u8), Just(2u8), Just(3u8)],
            0..200
        )
    ) {
        let mut s = SmpState::init(max_cpus).unwrap();
        for op in ops {
            match op {
                0 => { s.start_cpu(); },
                1 => { s.stop_cpu(); },
                2 => { s.global_lock(); },
                3 => { s.global_unlock(); },
                _ => {},
            }
            // SM1: bounds invariant
            prop_assert!(s.active_get() >= 1);
            prop_assert!(s.active_get() <= s.max_cpus_get());
        }
    }

    /// SM2+SM3: start then stop returns to original active count.
    #[test]
    fn start_stop_roundtrip(max_cpus in 2u32..=MAX_CPUS) {
        let mut s = SmpState::init(max_cpus).unwrap();
        let original = s;
        prop_assert_eq!(s.start_cpu(), OK);
        prop_assert_eq!(s.active_get(), 2);
        prop_assert_eq!(s.stop_cpu(), OK);
        prop_assert_eq!(s, original);
    }

    /// SM3: CPU 0 never stops (active never goes below 1).
    #[test]
    fn cpu0_never_stops(max_cpus in 1u32..=MAX_CPUS) {
        let mut s = SmpState::init(max_cpus).unwrap();
        // Try to stop when only CPU 0 is active
        prop_assert_eq!(s.stop_cpu(), EINVAL);
        prop_assert_eq!(s.active_get(), 1);
    }

    /// SM4: lock/unlock roundtrip preserves lock count.
    #[test]
    fn lock_unlock_roundtrip(
        max_cpus in 1u32..=MAX_CPUS,
        num_locks in 1u32..=100
    ) {
        let mut s = SmpState::init(max_cpus).unwrap();
        for _ in 0..num_locks {
            prop_assert_eq!(s.global_lock(), OK);
        }
        prop_assert_eq!(s.lock_count_get(), num_locks);
        for _ in 0..num_locks {
            prop_assert_eq!(s.global_unlock(), OK);
        }
        prop_assert_eq!(s.lock_count_get(), 0);
        prop_assert!(!s.is_locked());
    }

    /// Start all CPUs, then all rejects.
    #[test]
    fn start_all_then_reject(max_cpus in 1u32..=MAX_CPUS) {
        let mut s = SmpState::init(max_cpus).unwrap();
        for _ in 1..max_cpus {
            prop_assert_eq!(s.start_cpu(), OK);
        }
        prop_assert!(s.all_active());
        prop_assert_eq!(s.start_cpu(), EBUSY);
    }

    /// resume_cpu has same semantics as start_cpu.
    #[test]
    fn resume_same_as_start(max_cpus in 2u32..=MAX_CPUS) {
        let mut s1 = SmpState::init(max_cpus).unwrap();
        let mut s2 = SmpState::init(max_cpus).unwrap();
        prop_assert_eq!(s1.start_cpu(), s2.resume_cpu());
        prop_assert_eq!(s1, s2);
    }

    /// Conservation: active + inactive == max_cpus.
    #[test]
    fn conservation(
        max_cpus in 1u32..=MAX_CPUS,
        starts in 0u32..=15
    ) {
        let mut s = SmpState::init(max_cpus).unwrap();
        let actual_starts = starts.min(max_cpus - 1);
        for _ in 0..actual_starts {
            s.start_cpu();
        }
        prop_assert_eq!(s.active_get() + s.inactive_get(), max_cpus);
    }
}
