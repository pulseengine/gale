//! Property-based tests for the spinlock discipline model.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::unreachable
)]

use gale::error::*;
use gale::spinlock::SpinlockState;
use proptest::prelude::*;

proptest! {
    /// Init always creates a valid unlocked spinlock.
    #[test]
    fn init_always_valid(_dummy in 0u32..1) {
        let s = SpinlockState::init();
        prop_assert!(s.is_free());
        prop_assert!(!s.is_held());
        prop_assert_eq!(s.nest_depth(), 0);
    }

    /// SL1: acquire on free lock always succeeds.
    #[test]
    fn acquire_free_always_succeeds(tid in 0u32..=10000) {
        let mut s = SpinlockState::init();
        prop_assert_eq!(s.acquire(tid), OK);
        prop_assert!(s.is_owner(tid));
        prop_assert_eq!(s.nest_depth(), 1);
    }

    /// SL2: release by non-owner always fails.
    #[test]
    fn release_non_owner_fails(owner in 0u32..=10000, other in 0u32..=10000) {
        prop_assume!(owner != other);
        let mut s = SpinlockState::init();
        s.acquire(owner);
        prop_assert_eq!(s.release(other), EPERM);
        prop_assert!(s.is_owner(owner));
        prop_assert_eq!(s.nest_depth(), 1);
    }

    /// SL1+SL4: acquire-release roundtrip preserves state.
    #[test]
    fn acquire_release_roundtrip(tid in 0u32..=10000) {
        let mut s = SpinlockState::init();
        let original = s;
        s.acquire(tid);
        s.release(tid);
        prop_assert_eq!(s, original);
    }

    /// SL3: N nested acquires then N releases returns to unlocked.
    #[test]
    fn nested_n_acquires_n_releases(tid in 0u32..=10000, n in 1u32..=50) {
        let mut s = SpinlockState::init();
        for _ in 0..n {
            prop_assert_eq!(s.acquire_nested(tid), OK);
        }
        prop_assert_eq!(s.nest_depth(), n);
        prop_assert!(s.is_owner(tid));

        for i in 0..n {
            prop_assert_eq!(s.release(tid), OK);
            if i < n - 1 {
                prop_assert!(s.is_held());
                prop_assert_eq!(s.nest_depth(), n - 1 - i);
            }
        }
        prop_assert!(s.is_free());
        prop_assert_eq!(s.nest_depth(), 0);
    }

    /// SL5: double-acquire without nesting always returns EBUSY.
    #[test]
    fn double_acquire_returns_ebusy(tid in 0u32..=10000) {
        let mut s = SpinlockState::init();
        s.acquire(tid);
        prop_assert_eq!(s.acquire(tid), EBUSY);
        prop_assert_eq!(s.nest_depth(), 1);
    }

    /// Invariant: owner.is_some() iff nest_count > 0 after arbitrary op sequence.
    #[test]
    fn invariant_owner_nest_consistency(
        tid in 0u32..=100,
        ops in proptest::collection::vec(
            prop_oneof![
                Just(0u8), // acquire
                Just(1u8), // acquire_nested
                Just(2u8), // release
            ],
            0..50
        )
    ) {
        let mut s = SpinlockState::init();
        for op in ops {
            match op {
                0 => { s.acquire(tid); }
                1 => { s.acquire_nested(tid); }
                2 => { s.release(tid); }
                _ => unreachable!(),
            }
            // Invariant check
            prop_assert_eq!(
                s.owner_get().is_some(),
                s.nest_depth() > 0,
                "owner/nest_count inconsistency: owner={:?}, nest={}",
                s.owner_get(), s.nest_depth()
            );
        }
    }

    /// acquire_check is consistent with acquire result.
    #[test]
    fn acquire_check_consistent(tid1 in 0u32..=100, tid2 in 0u32..=100) {
        let mut s = SpinlockState::init();
        s.acquire(tid1);
        let check = s.acquire_check(tid2);
        // acquire_check should return false (lock is held)
        prop_assert!(!check);
    }
}
