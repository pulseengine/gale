//! Differential equivalence tests — Fatal (FFI vs Model).
//!
//! Verifies that the FFI fatal error classification functions produce
//! the same results as the Verus-verified model functions in gale::fatal.

#![allow(
    clippy::shadow_unrelated,
    clippy::indexing_slicing,
    clippy::bool_to_int_with_if,
    clippy::unwrap_used
)]

use gale::error::*;
use gale::fatal::{FatalError, FatalContext, FatalReason, RecoveryAction};

const FATAL_ACTION_ABORT_THREAD: u8 = 0;
const FATAL_ACTION_HALT: u8 = 1;
const FATAL_ACTION_IGNORE: u8 = 2;

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Replica of gale_k_fatal_decide.
fn ffi_fatal_decide(reason: u32, is_isr: bool, test_mode: bool) -> (u8, i32) {
    // Validate reason code
    if reason > 4 {
        return (FATAL_ACTION_HALT, EINVAL);
    }

    let action = if test_mode {
        if is_isr {
            if reason == 2 {
                // STACK_CHECK_FAIL — abort even in ISR
                FATAL_ACTION_ABORT_THREAD
            } else {
                FATAL_ACTION_IGNORE
            }
        } else {
            FATAL_ACTION_ABORT_THREAD
        }
    } else {
        // Production mode
        if reason == 4 {
            // KERNEL_PANIC
            FATAL_ACTION_HALT
        } else if reason == 2 {
            // STACK_CHECK_FAIL
            FATAL_ACTION_ABORT_THREAD
        } else if is_isr {
            FATAL_ACTION_HALT
        } else {
            FATAL_ACTION_ABORT_THREAD
        }
    };

    (action, 0)
}

/// Replica of gale_fatal_classify (legacy wrapper).
fn ffi_fatal_classify(reason: u32, is_isr: bool, test_mode: bool) -> i32 {
    let (action, ret) = ffi_fatal_decide(reason, is_isr, test_mode);
    if ret != 0 { ret } else { action as i32 }
}

// =====================================================================
// FFI-to-model reason mapping helper
// =====================================================================

fn reason_from_u32(r: u32) -> Option<FatalReason> {
    match r {
        0 => Some(FatalReason::CpuException),
        1 => Some(FatalReason::SpuriousIrq),
        2 => Some(FatalReason::StackCheckFail),
        3 => Some(FatalReason::KernelOops),
        4 => Some(FatalReason::KernelPanic),
        _ => None,
    }
}

fn model_classify(reason: FatalReason, context: FatalContext, test_mode: bool) -> RecoveryAction {
    let err = FatalError {
        reason,
        context,
        test_mode,
    };
    err.classify()
}

fn action_to_u8(a: RecoveryAction) -> u8 {
    match a {
        RecoveryAction::AbortThread => FATAL_ACTION_ABORT_THREAD,
        RecoveryAction::Halt => FATAL_ACTION_HALT,
        RecoveryAction::Ignore => FATAL_ACTION_IGNORE,
    }
}

// =====================================================================
// Differential tests: fatal decide
// =====================================================================

#[test]
fn fatal_decide_ffi_matches_model_exhaustive() {
    for reason in 0u32..=5 {
        for is_isr in [false, true] {
            for test_mode in [false, true] {
                let (ffi_action, ffi_ret) = ffi_fatal_decide(reason, is_isr, test_mode);

                if reason > 4 {
                    assert_eq!(ffi_ret, EINVAL,
                        "invalid reason: {reason}");
                    assert_eq!(ffi_action, FATAL_ACTION_HALT);
                    continue;
                }

                let model_reason = reason_from_u32(reason).unwrap();
                let context = if is_isr { FatalContext::Isr } else { FatalContext::Thread };
                let model_action = model_classify(model_reason, context, test_mode);
                let expected_action = action_to_u8(model_action);

                assert_eq!(ffi_action, expected_action,
                    "action mismatch: reason={reason}, isr={is_isr}, test={test_mode}");
                assert_eq!(ffi_ret, 0, "ret should be 0 for valid reason");
            }
        }
    }
}

// =====================================================================
// Differential tests: fatal classify (legacy)
// =====================================================================

#[test]
fn fatal_classify_ffi_matches_decide() {
    for reason in 0u32..=5 {
        for is_isr in [false, true] {
            for test_mode in [false, true] {
                let classify_result = ffi_fatal_classify(reason, is_isr, test_mode);
                let (action, ret) = ffi_fatal_decide(reason, is_isr, test_mode);

                if ret != 0 {
                    assert_eq!(classify_result, ret);
                } else {
                    assert_eq!(classify_result, action as i32);
                }
            }
        }
    }
}

// =====================================================================
// Property: FT2 — kernel panic always halts (production)
// =====================================================================

#[test]
fn fatal_kernel_panic_always_halts_production() {
    for is_isr in [false, true] {
        let (action, ret) = ffi_fatal_decide(4, is_isr, false);
        assert_eq!(ret, 0);
        assert_eq!(action, FATAL_ACTION_HALT,
            "FT2: KERNEL_PANIC must halt in production");
    }
}

// =====================================================================
// Property: FT3 — test mode ISR non-stack faults are ignored
// =====================================================================

#[test]
fn fatal_test_mode_isr_ignores_non_stack() {
    // reasons 0,1,3 in ISR + test_mode should be IGNORE
    for reason in [0u32, 1, 3] {
        let (action, _) = ffi_fatal_decide(reason, true, true);
        assert_eq!(action, FATAL_ACTION_IGNORE,
            "test mode ISR reason={reason} should IGNORE");
    }
    // reason 2 (STACK_CHECK_FAIL) in ISR + test_mode should ABORT
    let (action, _) = ffi_fatal_decide(2, true, true);
    assert_eq!(action, FATAL_ACTION_ABORT_THREAD,
        "test mode ISR STACK_CHECK_FAIL should ABORT");
}
