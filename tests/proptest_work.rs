//! Property-based tests for the work queue model.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::shadow_unrelated
)]

use gale::error::*;
use gale::work::*;
use proptest::prelude::*;

proptest! {
    /// WK1: init always produces idle state.
    #[test]
    fn init_always_idle(_seed in 0u32..1000) {
        let w = WorkItem::init();
        prop_assert!(w.is_idle());
        prop_assert_eq!(w.flags, 0);
        prop_assert_eq!(w.busy_get(), 0);
    }

    /// WK3: submit while canceling always returns EBUSY.
    #[test]
    fn submit_while_canceling_rejects(cycles in 1u32..20) {
        let mut w = WorkItem::init();
        // Get into canceling state
        w.submit();
        w.start_running();
        w.cancel();
        prop_assert!(w.is_canceling());

        // All subsequent submits should fail
        for _ in 0..cycles {
            prop_assert_eq!(w.submit(), EBUSY);
        }
    }

    /// WK4: submit is idempotent when already queued.
    #[test]
    fn submit_idempotent_when_queued(extra_submits in 1u32..50) {
        let mut w = WorkItem::init();
        prop_assert_eq!(w.submit(), 1); // first submit queues

        for _ in 0..extra_submits {
            prop_assert_eq!(w.submit(), 0); // idempotent
            prop_assert!(w.is_queued());
        }
    }

    /// Full lifecycle preserves idle state.
    #[test]
    fn full_lifecycle_roundtrip(cycles in 1u32..100) {
        let mut w = WorkItem::init();

        for _ in 0..cycles {
            prop_assert!(w.is_idle());
            prop_assert_eq!(w.submit(), 1);
            w.start_running();
            prop_assert!(w.is_running());
            w.finish_running();
        }
        prop_assert!(w.is_idle());
    }

    /// Cancel lifecycle: cancel during running -> finish_running -> finish_cancel -> idle.
    #[test]
    fn cancel_running_returns_to_idle(cycles in 1u32..50) {
        let mut w = WorkItem::init();

        for _ in 0..cycles {
            w.submit();
            w.start_running();
            w.cancel();
            prop_assert!(w.is_canceling());
            prop_assert!(w.is_running());

            w.finish_running();
            prop_assert!(w.is_canceling());

            w.finish_cancel();
            prop_assert!(w.is_idle());
        }
    }

    /// WK5: cancel always clears QUEUED.
    #[test]
    fn cancel_clears_queued(_seed in 0u32..100) {
        let mut w = WorkItem::init();
        w.submit();
        prop_assert!(w.is_queued());

        w.cancel();
        prop_assert!(!w.is_queued());
    }

    /// busy_get is consistent with individual flag checks.
    #[test]
    fn busy_get_consistent(_seed in 0u32..100) {
        let mut w = WorkItem::init();
        prop_assert_eq!(w.busy_get(), 0);

        w.submit();
        let busy = w.busy_get();
        prop_assert!(busy & FLAG_QUEUED != 0);
        prop_assert!(busy & FLAG_RUNNING == 0);

        w.start_running();
        let busy = w.busy_get();
        prop_assert!(busy & FLAG_RUNNING != 0);
        prop_assert!(busy & FLAG_QUEUED == 0);
    }
}
