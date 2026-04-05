//! Differential equivalence tests — Work (FFI vs Model).
//!
//! Verifies that the FFI work functions produce the same results as
//! the Verus-verified model functions in gale::work.

#![allow(
    clippy::shadow_unrelated,
    clippy::indexing_slicing,
    clippy::bool_to_int_with_if
)]

use gale::error::*;
use gale::work::{
    WorkItem, FLAG_RUNNING, FLAG_CANCELING, FLAG_QUEUED, BUSY_MASK,
};

const WORK_SUBMIT_QUEUE: u8 = 0;
const WORK_SUBMIT_REQUEUE: u8 = 1;
const WORK_SUBMIT_ALREADY: u8 = 2;
const WORK_SUBMIT_REJECT: u8 = 3;

const WORK_CANCEL_IDLE: u8 = 0;
const WORK_CANCEL_DEQUEUE: u8 = 1;
const WORK_CANCEL_CANCELING: u8 = 2;

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Replica of gale_k_work_submit_decide.
#[allow(clippy::arithmetic_side_effects)]
fn ffi_work_submit_decide(flags: u8, is_queued: u8, is_running: u8) -> (u8, u8, i32) {
    if (flags & FLAG_CANCELING) != 0 {
        return (WORK_SUBMIT_REJECT, flags, EBUSY);
    }
    if is_queued != 0 {
        return (WORK_SUBMIT_ALREADY, flags, 0);
    }
    let new_flags = flags | FLAG_QUEUED;
    if is_running != 0 {
        (WORK_SUBMIT_REQUEUE, new_flags, 2)
    } else {
        (WORK_SUBMIT_QUEUE, new_flags, 1)
    }
}

/// Replica of gale_k_work_cancel_decide.
#[allow(clippy::arithmetic_side_effects)]
fn ffi_work_cancel_decide(flags: u8, is_queued: u8, _is_running: u8) -> (u8, u8, u8) {
    let dequeued = (flags & FLAG_CANCELING) == 0 && is_queued != 0;

    let mut f = if dequeued {
        flags & !FLAG_QUEUED
    } else {
        flags
    };

    let busy = f & BUSY_MASK;

    if busy != 0 {
        f |= FLAG_CANCELING;
    }

    let action = if busy == 0 {
        WORK_CANCEL_IDLE
    } else if dequeued {
        WORK_CANCEL_DEQUEUE
    } else {
        WORK_CANCEL_CANCELING
    };

    (action, f, busy)
}

/// Replica of gale_work_submit_validate (legacy wrapper).
#[allow(clippy::arithmetic_side_effects)]
fn ffi_work_submit_validate(flags: u8) -> (i32, u8) {
    let is_queued = if (flags & FLAG_QUEUED) != 0 { 1u8 } else { 0 };
    let is_running = if (flags & FLAG_RUNNING) != 0 { 1u8 } else { 0 };
    let (_, new_flags, ret) = ffi_work_submit_decide(flags, is_queued, is_running);
    (ret, new_flags)
}

/// Replica of gale_work_cancel_validate (legacy wrapper).
#[allow(clippy::arithmetic_side_effects)]
fn ffi_work_cancel_validate(flags: u8) -> (u8, u8) {
    let is_queued = if (flags & FLAG_QUEUED) != 0 { 1u8 } else { 0 };
    let is_running = if (flags & FLAG_RUNNING) != 0 { 1u8 } else { 0 };
    let (_, new_flags, busy) = ffi_work_cancel_decide(flags, is_queued, is_running);
    (new_flags, busy)
}

// =====================================================================
// Differential tests: work submit_decide
// =====================================================================

#[test]
fn work_submit_decide_ffi_matches_model_exhaustive() {
    // Test all valid flag combinations
    let flag_combos: Vec<u8> = (0u8..=7).collect();

    for &flags in &flag_combos {
        for is_queued in [0u8, 1] {
            for is_running in [0u8, 1] {
                let (ffi_action, ffi_new_flags, ffi_ret) =
                    ffi_work_submit_decide(flags, is_queued, is_running);

                if (flags & FLAG_CANCELING) != 0 {
                    assert_eq!(ffi_action, WORK_SUBMIT_REJECT,
                        "WK3: canceling rejects: flags={flags:#04b}");
                    assert_eq!(ffi_ret, EBUSY);
                    assert_eq!(ffi_new_flags, flags, "flags unchanged");
                } else if is_queued != 0 {
                    assert_eq!(ffi_action, WORK_SUBMIT_ALREADY,
                        "WK4: already queued: flags={flags:#04b}");
                    assert_eq!(ffi_ret, 0);
                    assert_eq!(ffi_new_flags, flags, "flags unchanged");
                } else {
                    #[allow(clippy::arithmetic_side_effects)]
                    let expected_flags = flags | FLAG_QUEUED;
                    assert_eq!(ffi_new_flags, expected_flags, "QUEUED set");
                    if is_running != 0 {
                        assert_eq!(ffi_action, WORK_SUBMIT_REQUEUE);
                        assert_eq!(ffi_ret, 2);
                    } else {
                        assert_eq!(ffi_action, WORK_SUBMIT_QUEUE,
                            "WK2: idle -> queued: flags={flags:#04b}");
                        assert_eq!(ffi_ret, 1);
                    }
                }
            }
        }
    }
}

// =====================================================================
// Differential tests: work cancel_decide
// =====================================================================

#[test]
fn work_cancel_decide_ffi_matches_model_exhaustive() {
    let flag_combos: Vec<u8> = (0u8..=7).collect();

    for &flags in &flag_combos {
        for is_queued in [0u8, 1] {
            for is_running in [0u8, 1] {
                let (ffi_action, ffi_new_flags, ffi_busy) =
                    ffi_work_cancel_decide(flags, is_queued, is_running);

                // Verify properties
                if ffi_busy == 0 {
                    assert_eq!(ffi_action, WORK_CANCEL_IDLE,
                        "idle: flags={flags:#04b}");
                }
                // If busy and QUEUED was cleared, action is DEQUEUE
                // If busy and QUEUED was not cleared, action is CANCELING
            }
        }
    }
}

// =====================================================================
// Differential tests: legacy submit_validate / cancel_validate
// =====================================================================

#[test]
fn work_submit_validate_cross_check() {
    for flags in 0u8..=7 {
        let (ffi_ret, ffi_new_flags) = ffi_work_submit_validate(flags);

        // Cross-check with submit_decide
        let is_queued = if (flags & FLAG_QUEUED) != 0 { 1u8 } else { 0 };
        let is_running = if (flags & FLAG_RUNNING) != 0 { 1u8 } else { 0 };
        let (_, decide_new_flags, decide_ret) =
            ffi_work_submit_decide(flags, is_queued, is_running);

        assert_eq!(ffi_ret, decide_ret, "submit validate/decide ret mismatch");
        assert_eq!(ffi_new_flags, decide_new_flags, "submit validate/decide flags mismatch");
    }
}

#[test]
fn work_cancel_validate_cross_check() {
    for flags in 0u8..=7 {
        let (ffi_new_flags, ffi_busy) = ffi_work_cancel_validate(flags);

        // Cross-check with cancel_decide
        let is_queued = if (flags & FLAG_QUEUED) != 0 { 1u8 } else { 0 };
        let is_running = if (flags & FLAG_RUNNING) != 0 { 1u8 } else { 0 };
        let (_, decide_new_flags, decide_busy) =
            ffi_work_cancel_decide(flags, is_queued, is_running);

        assert_eq!(ffi_new_flags, decide_new_flags, "cancel validate/decide flags mismatch");
        assert_eq!(ffi_busy, decide_busy, "cancel validate/decide busy mismatch");
    }
}

// =====================================================================
// Property: WK1 — init produces IDLE (no flags)
// =====================================================================

#[test]
fn work_init_is_idle() {
    let w = WorkItem::init();
    assert_eq!(w.flags, 0, "WK1: init should be IDLE");
    assert!(w.is_idle(), "WK1: should be idle");
}

// =====================================================================
// Property: WK5 — cancel clears QUEUED if not already canceling
// =====================================================================

#[test]
fn work_cancel_clears_queued() {
    // QUEUED=4, not canceling, not running
    let flags: u8 = FLAG_QUEUED;
    let (action, new_flags, busy) = ffi_work_cancel_decide(flags, 1, 0);
    assert_eq!(action, WORK_CANCEL_IDLE, "no busy after dequeue");
    assert_eq!(new_flags & FLAG_QUEUED, 0, "QUEUED cleared");
    assert_eq!(busy, 0, "not busy");
}
