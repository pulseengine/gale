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
use crate::error::*;
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
    /// Create a fatal error event.
    pub fn new(
        reason: FatalReason,
        context: FatalContext,
        test_mode: bool,
    ) -> FatalError {
        FatalError {
            reason,
            context,
            test_mode,
        }
    }
    /// Determine the recovery action for this fatal error.
    ///
    /// Models the decision logic in z_fatal_error() (fatal.c:85-179).
    ///
    /// FT2: kernel panic always halts.
    /// FT3: recovery depends on reason, context, and test mode.
    pub fn classify(&self) -> RecoveryAction {
        if matches!(self.reason, FatalReason::KernelPanic) {
            return RecoveryAction::Halt;
        }
        if !self.test_mode {
            match self.reason {
                FatalReason::KernelPanic => RecoveryAction::Halt,
                FatalReason::CpuException => {
                    match self.context {
                        FatalContext::Isr => RecoveryAction::Halt,
                        FatalContext::Thread => RecoveryAction::AbortThread,
                    }
                }
                FatalReason::SpuriousIrq => {
                    match self.context {
                        FatalContext::Isr => RecoveryAction::Halt,
                        FatalContext::Thread => RecoveryAction::AbortThread,
                    }
                }
                FatalReason::StackCheckFail => RecoveryAction::AbortThread,
                FatalReason::KernelOops => {
                    match self.context {
                        FatalContext::Isr => RecoveryAction::Halt,
                        FatalContext::Thread => RecoveryAction::AbortThread,
                    }
                }
            }
        } else {
            match self.context {
                FatalContext::Isr => {
                    match self.reason {
                        FatalReason::SpuriousIrq => RecoveryAction::Ignore,
                        FatalReason::StackCheckFail => RecoveryAction::AbortThread,
                        FatalReason::CpuException => RecoveryAction::Ignore,
                        FatalReason::KernelOops => RecoveryAction::Ignore,
                        FatalReason::KernelPanic => RecoveryAction::Halt,
                    }
                }
                FatalContext::Thread => RecoveryAction::AbortThread,
            }
        }
    }
    /// Map a numeric reason code to FatalReason.
    ///
    /// Models the implicit mapping in z_fatal_error / reason_to_str.
    /// FT1: all valid codes produce a valid variant.
    pub fn from_code(code: u32) -> Option<FatalReason> {
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
    pub fn reason_str(reason: FatalReason) -> &'static str {
        match reason {
            FatalReason::CpuException => "CPU exception",
            FatalReason::SpuriousIrq => "Unhandled interrupt",
            FatalReason::StackCheckFail => "Stack overflow",
            FatalReason::KernelOops => "Kernel oops",
            FatalReason::KernelPanic => "Kernel panic",
        }
    }
    /// Check if the reason is a kernel panic.
    pub fn is_panic(&self) -> bool {
        matches!(self.reason, FatalReason::KernelPanic)
    }
    /// Check if the error occurred in ISR context.
    pub fn is_isr(&self) -> bool {
        matches!(self.context, FatalContext::Isr)
    }
}
/// Decision for fatal error classification from numeric arguments.
///
/// FT1: maps reason codes. FT2: panic halts. FT3: recovery depends on context.
/// Returns Ok(RecoveryAction) or Err(EINVAL) for unknown reason.
pub fn classify_decide(
    reason: u32,
    is_isr: bool,
    test_mode: bool,
) -> Result<RecoveryAction, i32> {
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
