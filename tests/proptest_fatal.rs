//! Property-based tests for the fatal error model.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::error::*;
use gale::fatal::*;
use proptest::prelude::*;

fn arb_reason() -> impl Strategy<Value = FatalReason> {
    prop_oneof![
        Just(FatalReason::CpuException),
        Just(FatalReason::SpuriousIrq),
        Just(FatalReason::StackCheckFail),
        Just(FatalReason::KernelOops),
        Just(FatalReason::KernelPanic),
    ]
}

fn arb_context() -> impl Strategy<Value = FatalContext> {
    prop_oneof![Just(FatalContext::Thread), Just(FatalContext::Isr),]
}

proptest! {
    /// FT1: from_code always returns Some for valid codes (0..=4).
    #[test]
    fn valid_codes_always_some(code in 0u32..=4) {
        prop_assert!(FatalError::from_code(code).is_some());
    }

    /// FT1: from_code returns None for invalid codes.
    #[test]
    fn invalid_codes_always_none(code in 5u32..=u32::MAX) {
        prop_assert!(FatalError::from_code(code).is_none());
    }

    /// FT2: kernel panic in production always halts.
    #[test]
    fn panic_production_always_halts(context in arb_context()) {
        let e = FatalError::new(FatalReason::KernelPanic, context, false);
        prop_assert_eq!(e.classify(), RecoveryAction::Halt);
    }

    /// FT3: thread-context faults in production always abort thread (except panic).
    #[test]
    fn thread_production_aborts(reason in arb_reason()) {
        let e = FatalError::new(reason, FatalContext::Thread, false);
        let action = e.classify();
        if reason == FatalReason::KernelPanic {
            prop_assert_eq!(action, RecoveryAction::Halt);
        } else {
            prop_assert_eq!(action, RecoveryAction::AbortThread);
        }
    }

    /// Stack check fail always results in AbortThread regardless of context or mode.
    #[test]
    fn stack_check_always_aborts(context in arb_context(), test_mode: bool) {
        let e = FatalError::new(FatalReason::StackCheckFail, context, test_mode);
        prop_assert_eq!(e.classify(), RecoveryAction::AbortThread);
    }

    /// FT4: all valid reason codes produce distinct variants.
    #[test]
    fn reason_codes_distinct(a in 0u32..=4, b in 0u32..=4) {
        if a != b {
            prop_assert_ne!(FatalError::from_code(a), FatalError::from_code(b));
        }
    }

    /// classify always returns a valid RecoveryAction.
    #[test]
    fn classify_always_valid(reason in arb_reason(), context in arb_context(), test_mode: bool) {
        let e = FatalError::new(reason, context, test_mode);
        let action = e.classify();
        // Just verify it doesn't panic and returns one of the three variants
        let _valid = matches!(action, RecoveryAction::AbortThread | RecoveryAction::Halt | RecoveryAction::Ignore);
        prop_assert!(_valid);
    }
}
