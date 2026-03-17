//! Integration tests for the fatal error model.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing
)]

use gale::fatal::*;

#[test]
fn from_code_valid_codes() {
    assert_eq!(FatalError::from_code(0), Some(FatalReason::CpuException));
    assert_eq!(FatalError::from_code(1), Some(FatalReason::SpuriousIrq));
    assert_eq!(FatalError::from_code(2), Some(FatalReason::StackCheckFail));
    assert_eq!(FatalError::from_code(3), Some(FatalReason::KernelOops));
    assert_eq!(FatalError::from_code(4), Some(FatalReason::KernelPanic));
}

#[test]
fn from_code_invalid() {
    assert_eq!(FatalError::from_code(5), None);
    assert_eq!(FatalError::from_code(255), None);
    assert_eq!(FatalError::from_code(u32::MAX), None);
}

#[test]
fn panic_always_halts_production() {
    let e1 = FatalError::new(FatalReason::KernelPanic, FatalContext::Thread, false);
    assert_eq!(e1.classify(), RecoveryAction::Halt);

    let e2 = FatalError::new(FatalReason::KernelPanic, FatalContext::Isr, false);
    assert_eq!(e2.classify(), RecoveryAction::Halt);
}

#[test]
fn thread_faults_abort_production() {
    let reasons = [
        FatalReason::CpuException,
        FatalReason::SpuriousIrq,
        FatalReason::StackCheckFail,
        FatalReason::KernelOops,
    ];
    for reason in &reasons {
        let e = FatalError::new(*reason, FatalContext::Thread, false);
        assert_eq!(
            e.classify(),
            RecoveryAction::AbortThread,
            "Thread-context {reason:?} should abort thread in production"
        );
    }
}

#[test]
fn isr_faults_halt_production() {
    let isr_halt_reasons = [
        FatalReason::CpuException,
        FatalReason::SpuriousIrq,
        FatalReason::KernelOops,
    ];
    for reason in &isr_halt_reasons {
        let e = FatalError::new(*reason, FatalContext::Isr, false);
        assert_eq!(
            e.classify(),
            RecoveryAction::Halt,
            "ISR-context {reason:?} should halt in production"
        );
    }
}

#[test]
fn stack_check_always_aborts() {
    // Stack check fail always aborts the thread, even in ISR context
    let e1 = FatalError::new(FatalReason::StackCheckFail, FatalContext::Thread, false);
    assert_eq!(e1.classify(), RecoveryAction::AbortThread);

    let e2 = FatalError::new(FatalReason::StackCheckFail, FatalContext::Isr, false);
    assert_eq!(e2.classify(), RecoveryAction::AbortThread);
}

#[test]
fn test_mode_isr_ignores() {
    let ignore_reasons = [
        FatalReason::CpuException,
        FatalReason::SpuriousIrq,
        FatalReason::KernelOops,
        FatalReason::KernelPanic,
    ];
    for reason in &ignore_reasons {
        let e = FatalError::new(*reason, FatalContext::Isr, true);
        assert_eq!(
            e.classify(),
            RecoveryAction::Ignore,
            "ISR-context {reason:?} should be ignored in test mode"
        );
    }
}

#[test]
fn test_mode_isr_stack_check_aborts() {
    let e = FatalError::new(FatalReason::StackCheckFail, FatalContext::Isr, true);
    assert_eq!(e.classify(), RecoveryAction::AbortThread);
}

#[test]
fn test_mode_thread_always_aborts() {
    let all_reasons = [
        FatalReason::CpuException,
        FatalReason::SpuriousIrq,
        FatalReason::StackCheckFail,
        FatalReason::KernelOops,
        FatalReason::KernelPanic,
    ];
    for reason in &all_reasons {
        let e = FatalError::new(*reason, FatalContext::Thread, true);
        assert_eq!(
            e.classify(),
            RecoveryAction::AbortThread,
            "Thread-context {reason:?} should abort in test mode"
        );
    }
}

#[test]
fn reason_str_matches() {
    assert_eq!(
        FatalError::reason_str(FatalReason::CpuException),
        "CPU exception"
    );
    assert_eq!(
        FatalError::reason_str(FatalReason::SpuriousIrq),
        "Unhandled interrupt"
    );
    assert_eq!(
        FatalError::reason_str(FatalReason::StackCheckFail),
        "Stack overflow"
    );
    assert_eq!(
        FatalError::reason_str(FatalReason::KernelOops),
        "Kernel oops"
    );
    assert_eq!(
        FatalError::reason_str(FatalReason::KernelPanic),
        "Kernel panic"
    );
}

#[test]
fn is_panic_checks() {
    let panic = FatalError::new(FatalReason::KernelPanic, FatalContext::Thread, false);
    assert!(panic.is_panic());

    let oops = FatalError::new(FatalReason::KernelOops, FatalContext::Thread, false);
    assert!(!oops.is_panic());
}

#[test]
fn is_isr_checks() {
    let isr = FatalError::new(FatalReason::CpuException, FatalContext::Isr, false);
    assert!(isr.is_isr());

    let thread = FatalError::new(FatalReason::CpuException, FatalContext::Thread, false);
    assert!(!thread.is_isr());
}

#[test]
fn clone_and_eq() {
    let e1 = FatalError::new(FatalReason::KernelOops, FatalContext::Thread, false);
    let e2 = e1;
    assert_eq!(e1, e2);

    let e3 = FatalError::new(FatalReason::KernelOops, FatalContext::Isr, false);
    assert_ne!(e1, e3);
}

#[test]
fn reason_codes_distinct() {
    let codes: Vec<Option<FatalReason>> = (0..=4).map(FatalError::from_code).collect();
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            assert_ne!(codes[i], codes[j], "codes {i} and {j} should differ");
        }
    }
}
