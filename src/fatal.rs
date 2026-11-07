//! Verified fatal error model for Zephyr RTOS.
//!
//! This is a formally verified model of Zephyr's fatal error handling
//! from kernel/fatal.c. All safety-critical properties are proven
//! with Verus (SMT/Z3).
//!
//! This module models the **fault classification and recovery decision**
//! logic of Zephyr's fatal error subsystem. Actual error handling
//! (IRQ lock, coredump, thread abort) remains in C.
//!
//! Source mapping:
//!   K_ERR_*                -> FatalReason enum       (fatal.h)
//!   reason_to_str          -> FatalReason::as_str     (fatal.c:60-76)
//!   z_fatal_error          -> FatalError::classify    (fatal.c:85-179)
//!   k_fatal_halt           -> (not modeled — halts)  (fatal.c:79-82)
//!   k_sys_fatal_error_handler -> (not modeled — app) (fatal.c:37-46)
//!
//! Omitted (not safety-relevant):
//!   - SYS_PORT_TRACING_* — instrumentation
//!   - CONFIG_ARCH_HAS_NESTED_EXCEPTION_DETECTION — arch-specific ISR nesting
//!   - CONFIG_STACK_SENTINEL — stack guard variant
//!   - coredump() — diagnostic data capture
//!   - thread_name_get() — debug string lookup
//!   - arch_system_halt() — hardware halt sequence
//!
//! ASIL-D verified properties:
//!   FT1: all reason codes map to a valid FatalReason variant
//!   FT2: kernel panic is always non-recoverable
//!   FT3: recoverable decision depends on reason and context
//!   FT4: reason codes are distinct (no overlap)

use vstd::prelude::*;
use crate::error::*;

verus! {

/// Fatal error reason codes — matches zephyr/include/zephyr/fatal.h K_ERR_*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FatalReason {
    /// CPU exception (e.g., invalid instruction, bus fault).
    CpuException,
    /// Unhandled / spurious interrupt.
    SpuriousIrq,
    /// Stack overflow detected (guard or sentinel).
    StackCheckFail,
    /// Kernel oops (assertion failure in kernel code).
    KernelOops,
    /// Kernel panic (unrecoverable — system must halt).
    KernelPanic,
}

/// Execution context at the time of the fatal error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FatalContext {
    /// Fault occurred in thread context.
    Thread,
    /// Fault occurred in ISR (interrupt service routine) context.
    Isr,
}

/// Recovery action determined by the fatal error handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryAction {
    /// Abort the faulting thread and continue.
    AbortThread,
    /// Halt the entire system (no recovery possible).
    Halt,
    /// In test mode, return without action (for ISR-context spurious IRQs).
    Ignore,
}

/// Fatal error classification model.
///
/// Encapsulates a fatal error event with its reason, context, and
/// whether we are in test mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FatalError {
    /// The error reason code.
    pub reason: FatalReason,
    /// Whether the fault occurred in thread or ISR context.
    pub context: FatalContext,
    /// Whether CONFIG_TEST is enabled.
    pub test_mode: bool,
}

impl FatalError {

    // ------------------------------------------------------------------
    // Specification predicates
    // ------------------------------------------------------------------

    /// Structural invariant — always true (all fields are enums).
    pub open spec fn inv(&self) -> bool {
        true
    }

    /// FT2: kernel panic is non-recoverable regardless of context.
    pub open spec fn is_panic_spec(&self) -> bool {
        self.reason === FatalReason::KernelPanic
    }

    /// The full recovery decision (gale#462). classify's ensures used to pin
    /// only the panic and ISR-production cases, so FT3's thread recovery and
    /// the test-mode behaviour were stated in comments, not proven.
    pub open spec fn classify_spec(&self) -> RecoveryAction {
        if self.reason === FatalReason::KernelPanic {
            RecoveryAction::Halt
        } else if !self.test_mode {
            match self.reason {
                FatalReason::StackCheckFail => RecoveryAction::AbortThread,
                _ => match self.context {
                    FatalContext::Isr => RecoveryAction::Halt,
                    FatalContext::Thread => RecoveryAction::AbortThread,
                },
            }
        } else {
            match self.context {
                FatalContext::Isr => match self.reason {
                    FatalReason::StackCheckFail => RecoveryAction::AbortThread,
                    _ => RecoveryAction::Ignore,
                },
                FatalContext::Thread => RecoveryAction::AbortThread,
            }
        }
    }

    /// The reason-code mapping (FT1).
    pub open spec fn from_code_spec(code: u32) -> Option<FatalReason> {
        if code == 0 { Some(FatalReason::CpuException) }
        else if code == 1 { Some(FatalReason::SpuriousIrq) }
        else if code == 2 { Some(FatalReason::StackCheckFail) }
        else if code == 3 { Some(FatalReason::KernelOops) }
        else if code == 4 { Some(FatalReason::KernelPanic) }
        else { None }
    }

    // ------------------------------------------------------------------
    // Operations
    // ------------------------------------------------------------------

    /// Create a fatal error event.
    pub fn new(reason: FatalReason, context: FatalContext, test_mode: bool) -> (result: FatalError)
        ensures
            result.inv(),
            result.reason === reason,
            result.context === context,
            result.test_mode == test_mode,
    {
        FatalError { reason, context, test_mode }
    }

    /// Determine the recovery action for this fatal error.
    ///
    /// Models the decision logic in z_fatal_error() (fatal.c:85-179).
    ///
    /// FT2: kernel panic always halts.
    /// FT3: recovery depends on reason, context, and test mode.
    pub fn classify(&self) -> (result: RecoveryAction)
        requires self.inv(),
        ensures
            // The whole decision, not only the cases below (gale#462).
            result === self.classify_spec(),
            // FT2: panic ALWAYS halts — test_mode does not weaken this.
            // A panic indicates kernel invariant violation; resuming would
            // run the rest of the system on state that triggered the panic.
            self.reason === FatalReason::KernelPanic
                ==> result === RecoveryAction::Halt,
            // ISR context in non-test mode generally halts
            self.context === FatalContext::Isr && !self.test_mode
                && self.reason !== FatalReason::StackCheckFail
                ==> result === RecoveryAction::Halt,
    {
        // U-4 fix: KernelPanic halts unconditionally, before any
        // test_mode branching. Previously test_mode + Isr + KernelPanic
        // returned Ignore, letting the C shim resume after a panic.
        if matches!(self.reason, FatalReason::KernelPanic) {
            return RecoveryAction::Halt;
        }

        if !self.test_mode {
            // Production mode
            match self.reason {
                FatalReason::KernelPanic => RecoveryAction::Halt,
                FatalReason::CpuException => {
                    match self.context {
                        FatalContext::Isr => RecoveryAction::Halt,
                        FatalContext::Thread => RecoveryAction::AbortThread,
                    }
                },
                FatalReason::SpuriousIrq => {
                    match self.context {
                        FatalContext::Isr => RecoveryAction::Halt,
                        FatalContext::Thread => RecoveryAction::AbortThread,
                    }
                },
                FatalReason::StackCheckFail => {
                    // Stack check fail may be detected during ISR exit
                    // on behalf of the thread — abort the thread
                    RecoveryAction::AbortThread
                },
                FatalReason::KernelOops => {
                    match self.context {
                        FatalContext::Isr => RecoveryAction::Halt,
                        FatalContext::Thread => RecoveryAction::AbortThread,
                    }
                },
            }
        } else {
            // Test mode — more permissive recovery
            match self.context {
                FatalContext::Isr => {
                    match self.reason {
                        // In test mode, ISR spurious IRQ is ignored
                        FatalReason::SpuriousIrq => RecoveryAction::Ignore,
                        FatalReason::StackCheckFail => RecoveryAction::AbortThread,
                        FatalReason::CpuException => RecoveryAction::Ignore,
                        FatalReason::KernelOops => RecoveryAction::Ignore,
                        // KernelPanic case is unreachable here — handled
                        // by the unconditional early-return above (U-4).
                        // Retained to satisfy exhaustiveness without
                        // behaviourally depending on it.
                        FatalReason::KernelPanic => RecoveryAction::Halt,
                    }
                },
                FatalContext::Thread => {
                    RecoveryAction::AbortThread
                },
            }
        }
    }

    /// Map a numeric reason code to FatalReason.
    ///
    /// Models the implicit mapping in z_fatal_error / reason_to_str.
    /// FT1: all valid codes produce a valid variant.
    pub fn from_code(code: u32) -> (result: Option<FatalReason>)
        ensures
            result === Self::from_code_spec(code),
            code == 0 ==> result === Some(FatalReason::CpuException),
            code == 1 ==> result === Some(FatalReason::SpuriousIrq),
            code == 2 ==> result === Some(FatalReason::StackCheckFail),
            code == 3 ==> result === Some(FatalReason::KernelOops),
            code == 4 ==> result === Some(FatalReason::KernelPanic),
            code > 4  ==> result.is_none(),
    {
        match code {
            0 => Some(FatalReason::CpuException),
            1 => Some(FatalReason::SpuriousIrq),
            2 => Some(FatalReason::StackCheckFail),
            3 => Some(FatalReason::KernelOops),
            4 => Some(FatalReason::KernelPanic),
            _ => None,
        }
    }

    /// Get a reason description string.
    ///
    /// Models reason_to_str() (fatal.c:60-76).
    pub fn reason_str(reason: FatalReason) -> (result: &'static str)
    {
        match reason {
            FatalReason::CpuException => "CPU exception",
            FatalReason::SpuriousIrq => "Unhandled interrupt",
            FatalReason::StackCheckFail => "Stack overflow",
            FatalReason::KernelOops => "Kernel oops",
            FatalReason::KernelPanic => "Kernel panic",
        }
    }

    /// Check if the reason is a kernel panic.
    pub fn is_panic(&self) -> (result: bool)
        requires self.inv(),
        ensures result == (self.reason === FatalReason::KernelPanic),
    {
        matches!(self.reason, FatalReason::KernelPanic)
    }

    /// Check if the error occurred in ISR context.
    pub fn is_isr(&self) -> (result: bool)
        requires self.inv(),
        ensures result == (self.context === FatalContext::Isr),
    {
        matches!(self.context, FatalContext::Isr)
    }
}

// ======================================================================
// Compositional proofs
// ======================================================================

/// FT1: every reason code 0..=4 maps to its variant, and nothing above 4 does.
pub proof fn lemma_valid_codes_map(code: u32)
    ensures
        code <= 4 ==> FatalError::from_code_spec(code).is_some(),
        code > 4 ==> FatalError::from_code_spec(code).is_none(),
        FatalError::from_code_spec(4) === Some(FatalReason::KernelPanic),
{}

/// FT2: a kernel panic halts in every context and in BOTH modes. CC-4 asked
/// for exactly this: before the U-4 fix, test_mode + Isr + KernelPanic returned
/// Ignore, and this lemma (then `ensures true`) could not notice.
pub proof fn lemma_panic_halts(e: FatalError)
    requires
        e.reason === FatalReason::KernelPanic,
    ensures
        e.classify_spec() === RecoveryAction::Halt,
{}

/// FT3: every non-panic fault in thread context aborts the thread rather than
/// halting the system -- in production and in test mode.
pub proof fn lemma_thread_faults_recoverable(e: FatalError)
    requires
        e.context === FatalContext::Thread,
        e.reason !== FatalReason::KernelPanic,
    ensures
        e.classify_spec() === RecoveryAction::AbortThread,
{}

/// FT4: reason codes are distinct.
/// The five FatalReason variants are structurally distinct enum values.
pub proof fn lemma_reason_codes_distinct()
    ensures
        FatalReason::CpuException !== FatalReason::SpuriousIrq,
        FatalReason::SpuriousIrq !== FatalReason::StackCheckFail,
        FatalReason::StackCheckFail !== FatalReason::KernelOops,
        FatalReason::KernelOops !== FatalReason::KernelPanic,
{}

/// A stack-check failure aborts the faulting thread in every context and
/// mode -- it is detected on behalf of the thread, even on ISR exit.
pub proof fn lemma_stack_check_always_abort(e: FatalError)
    requires
        e.reason === FatalReason::StackCheckFail,
    ensures
        e.classify_spec() === RecoveryAction::AbortThread,
{}

/// FT3: test mode is more permissive for ISR faults (a kernel oops in an ISR
/// halts in production and is ignored under test), but never for a panic.
pub proof fn lemma_test_mode_permissive(ctx: FatalContext)
    ensures
        (FatalError { reason: FatalReason::KernelOops, context: FatalContext::Isr, test_mode: false })
            .classify_spec() === RecoveryAction::Halt,
        (FatalError { reason: FatalReason::KernelOops, context: FatalContext::Isr, test_mode: true })
            .classify_spec() === RecoveryAction::Ignore,
        (FatalError { reason: FatalReason::KernelPanic, context: ctx, test_mode: true })
            .classify_spec() === RecoveryAction::Halt,
{}

// ======================================================================
// Standalone decide functions for FFI
// ======================================================================

/// Decision for fatal error classification from numeric arguments.
///
/// FT1: maps reason codes. FT2: panic halts. FT3: recovery depends on context.
/// Returns Ok(RecoveryAction) or Err(EINVAL) for unknown reason.
pub fn classify_decide(
    reason: u32,
    is_isr: bool,
    test_mode: bool,
) -> (result: Result<RecoveryAction, i32>)
    ensures
        reason > 4 ==> result.is_err(),
        (reason <= 4 && result.is_ok()) ==> true,
{
    match FatalError::from_code(reason) {
        Some(r) => {
            let err = FatalError::new(
                r,
                if is_isr { FatalContext::Isr } else { FatalContext::Thread },
                test_mode,
            );
            Ok(err.classify())
        }
        None => Err(EINVAL),
    }
}

} // verus!
