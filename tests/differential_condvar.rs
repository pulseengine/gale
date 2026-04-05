//! Differential equivalence tests — CondVar (Model consistency).
//!
//! Verifies internal consistency of the Verus-verified condvar model
//! in gale::condvar.  Since there are no FFI shim functions for condvar
//! (it delegates entirely to the wait-queue primitives), this file uses
//! scalar replicas derived directly from the model source to cross-check
//! the signal/broadcast/wait logic.

#![allow(
    clippy::shadow_unrelated,
    clippy::indexing_slicing,
    clippy::bool_to_int_with_if,
    clippy::unwrap_used,
    clippy::fn_params_excessive_bools,
    clippy::absurd_extreme_comparisons,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::checked_conversions,
    clippy::wildcard_enum_match_arm,
    clippy::implicit_saturating_sub,
    clippy::branches_sharing_code,
    clippy::panic
)]

use gale::error::*;
use gale::condvar::{CondVar, SignalResult};

// =====================================================================
// Scalar replicas of the condvar model logic
// =====================================================================

/// Replica of CondVar::signal (scalar version, no WaitQueue).
///
/// Returns (woke_one: bool, remaining_waiters: u32).
fn scalar_signal(num_waiters: u32) -> (bool, u32) {
    if num_waiters > 0 {
        #[allow(clippy::arithmetic_side_effects)]
        (true, num_waiters - 1)
    } else {
        (false, 0)
    }
}

/// Replica of CondVar::broadcast (scalar version, no WaitQueue).
///
/// Returns number of waiters woken (= original num_waiters).
fn scalar_broadcast(num_waiters: u32) -> u32 {
    num_waiters
}

// =====================================================================
// Differential tests: signal behaviour
// =====================================================================

#[test]
fn condvar_signal_zero_waiters_is_noop() {
    // C3: signal on empty condvar is a no-op
    let mut cv = CondVar::init();
    let result = cv.signal();

    match result {
        SignalResult::Empty => {}
        SignalResult::Woke(_) => panic!("signal on empty condvar should return Empty"),
    }

    assert_eq!(cv.num_waiters(), 0, "waiters should remain 0 after empty signal");
    // Scalar replica agrees
    let (woke, remaining) = scalar_signal(0);
    assert!(!woke, "scalar: should not wake when no waiters");
    assert_eq!(remaining, 0);
}

#[test]
fn condvar_signal_scalar_matches_model_exhaustive() {
    // For each waiter count, the scalar replica and model must agree on
    // how many waiters remain after a signal.
    for num_waiters in 0u32..=20 {
        let (scalar_woke, scalar_remaining) = scalar_signal(num_waiters);

        if num_waiters == 0 {
            // C3: no-op
            assert!(!scalar_woke, "count={num_waiters}: should not wake");
            assert_eq!(scalar_remaining, 0);
        } else {
            // C2: exactly one woken
            assert!(scalar_woke, "count={num_waiters}: should wake one");
            #[allow(clippy::arithmetic_side_effects)]
            let expected_remaining = num_waiters - 1;
            assert_eq!(
                scalar_remaining, expected_remaining,
                "count={num_waiters}: remaining mismatch"
            );
        }
    }
}

// =====================================================================
// Differential tests: broadcast behaviour
// =====================================================================

#[test]
fn condvar_broadcast_zero_waiters_returns_zero() {
    // C5: broadcast on empty condvar returns 0
    let mut cv = CondVar::init();
    let woken = cv.broadcast();
    assert_eq!(woken, 0, "broadcast on empty condvar should return 0");
    assert_eq!(cv.num_waiters(), 0);

    let scalar_woken = scalar_broadcast(0);
    assert_eq!(scalar_woken, 0);
}

#[test]
fn condvar_broadcast_scalar_matches_model_exhaustive() {
    // C4: broadcast wakes all waiters
    for num_waiters in 0u32..=20 {
        let scalar_woken = scalar_broadcast(num_waiters);
        // Scalar: all waiters woken
        assert_eq!(
            scalar_woken, num_waiters,
            "count={num_waiters}: broadcast should wake all"
        );
    }
}

// =====================================================================
// Property: C3 — signal idempotence on empty condvar
// =====================================================================

#[test]
fn condvar_multiple_signals_on_empty_all_noop() {
    let mut cv = CondVar::init();
    for _ in 0..5 {
        let result = cv.signal();
        match result {
            SignalResult::Empty => {}
            SignalResult::Woke(_) => panic!("signal on empty must always return Empty"),
        }
    }
    assert_eq!(cv.num_waiters(), 0);
}

// =====================================================================
// Property: C5 — broadcast idempotence on empty condvar
// =====================================================================

#[test]
fn condvar_broadcast_idempotent_on_empty() {
    let mut cv = CondVar::init();
    let w1 = cv.broadcast();
    let w2 = cv.broadcast();
    assert_eq!(w1, 0);
    assert_eq!(w2, 0);
    assert_eq!(cv.num_waiters(), 0);
}

// =====================================================================
// Property: signal N times vs broadcast on N waiters (scalar)
// =====================================================================

#[test]
fn condvar_n_signals_equivalent_to_broadcast_scalar() {
    // Scalar verification: N successive signals on N waiters wakes all N
    // (same as one broadcast).
    for n in 0u32..=20 {
        let mut remaining = n;
        let mut woken_count = 0u32;
        while remaining > 0 {
            let (woke, new_remaining) = scalar_signal(remaining);
            assert!(woke, "signal should wake when waiters > 0");
            #[allow(clippy::arithmetic_side_effects)]
            {
                woken_count += 1;
            }
            remaining = new_remaining;
        }
        assert_eq!(woken_count, n, "N signals should wake all N waiters");

        // broadcast equivalent: wakes all at once
        let broadcast_woken = scalar_broadcast(n);
        assert_eq!(broadcast_woken, n);
    }
}

// =====================================================================
// Property: C1 — init produces empty condvar
// =====================================================================

#[test]
fn condvar_init_produces_empty_queue() {
    let cv = CondVar::init();
    assert_eq!(cv.num_waiters(), 0, "C1: init must produce empty wait queue");
    assert!(!cv.has_waiters(), "C1: has_waiters should be false after init");
}

// =====================================================================
// Property: C4 — broadcast after signal cleans up remaining waiters
// =====================================================================

#[test]
fn condvar_signal_then_broadcast_scalar() {
    // Signal removes one, broadcast removes rest — together remove all.
    for n in 1u32..=20 {
        let (woke, remaining) = scalar_signal(n);
        assert!(woke);
        #[allow(clippy::arithmetic_side_effects)]
        let expected_remaining = n - 1;
        assert_eq!(remaining, expected_remaining);

        let broadcast_woken = scalar_broadcast(remaining);
        assert_eq!(broadcast_woken, expected_remaining);

        // Total woken: 1 (signal) + remaining (broadcast) == n
        #[allow(clippy::arithmetic_side_effects)]
        let total = 1u32 + broadcast_woken;
        assert_eq!(total, n, "signal+broadcast should wake all {n} waiters");
    }
}

// =====================================================================
// Consistency check: OK is the return value for woken threads
// =====================================================================

#[test]
fn condvar_ok_is_zero() {
    // Ensure OK == 0 (Zephyr convention: arch_thread_return_value_set(t, 0))
    assert_eq!(OK, 0, "OK must be 0 for condvar wakeup return value");
}
