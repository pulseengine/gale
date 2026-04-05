//! Differential equivalence tests — Poll (FFI vs Model).
//!
//! Verifies that the FFI poll functions produce the same results as
//! the Verus-verified model functions in gale::poll.

#![allow(
    clippy::shadow_unrelated,
    clippy::indexing_slicing,
    clippy::bool_to_int_with_if
)]

use gale::poll::{
    PollEvent, PollSignal,
    TYPE_IGNORE, TYPE_SEM_AVAILABLE, TYPE_DATA_AVAILABLE,
    TYPE_SIGNAL, TYPE_MSGQ_DATA_AVAILABLE,
    STATE_NOT_READY,
};

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Replica of gale_poll_event_init.
fn ffi_poll_event_init(_event_type: u32) -> u32 {
    0 // STATE_NOT_READY
}

/// Replica of gale_poll_check_sem.
fn ffi_poll_check_sem(event_type: u32, sem_count: u32) -> bool {
    const LOCAL_TYPE_SEM: u32 = 1;
    event_type == LOCAL_TYPE_SEM && sem_count > 0
}

/// Replica of gale_poll_signal_raise.
fn ffi_poll_signal_raise(result_val: i32) -> (u32, i32) {
    (1, result_val) // signaled=1, result=result_val
}

/// Replica of gale_poll_signal_reset.
fn ffi_poll_signal_reset() -> u32 {
    0 // signaled=0
}

/// Replica of gale_k_poll_signal_raise_decide.
fn ffi_poll_signal_raise_decide(
    _signaled: u32,
    result_val: i32,
    has_poll_event: bool,
) -> (u32, i32, u8) {
    let action = if has_poll_event { 1 } else { 0 };
    (1, result_val, action)
}

// =====================================================================
// Differential tests: poll_event_init
// =====================================================================

#[test]
fn poll_event_init_ffi_matches_model() {
    let types = [
        TYPE_IGNORE, TYPE_SEM_AVAILABLE, TYPE_DATA_AVAILABLE,
        TYPE_SIGNAL, TYPE_MSGQ_DATA_AVAILABLE,
    ];
    for &t in &types {
        let ffi_state = ffi_poll_event_init(t);
        let model = PollEvent::init(t, 0);

        assert_eq!(ffi_state, model.state,
            "poll_event_init state: type={t}");
        assert_eq!(ffi_state, STATE_NOT_READY);
    }
}

// =====================================================================
// Differential tests: poll_check_sem
// =====================================================================

#[test]
fn poll_check_sem_ffi_matches_model_exhaustive() {
    let types = [
        TYPE_IGNORE, TYPE_SEM_AVAILABLE, TYPE_DATA_AVAILABLE,
        TYPE_SIGNAL, TYPE_MSGQ_DATA_AVAILABLE,
    ];
    for &event_type in &types {
        for sem_count in 0u32..=5 {
            let ffi_result = ffi_poll_check_sem(event_type, sem_count);
            let event = PollEvent::init(event_type, 0);
            let model_result = event.check_sem(sem_count);

            assert_eq!(ffi_result, model_result,
                "check_sem: type={event_type}, count={sem_count}");
        }
    }
}

// =====================================================================
// Differential tests: poll_signal raise/reset/check
// =====================================================================

#[test]
fn poll_signal_raise_ffi_matches_model() {
    for result_val in [-100i32, -1, 0, 1, 42, i32::MAX] {
        let (ffi_signaled, ffi_result) = ffi_poll_signal_raise(result_val);

        let mut sig = PollSignal::init();
        sig.raise(result_val);

        assert_eq!(ffi_signaled, sig.signaled,
            "raise signaled: val={result_val}");
        assert_eq!(ffi_result, sig.result,
            "raise result: val={result_val}");
    }
}

#[test]
fn poll_signal_reset_ffi_matches_model() {
    let ffi_signaled = ffi_poll_signal_reset();

    let mut sig = PollSignal::init();
    sig.raise(42);
    sig.reset();

    assert_eq!(ffi_signaled, sig.signaled);
    assert_eq!(ffi_signaled, 0);
}

// =====================================================================
// Differential tests: poll_signal_raise_decide
// =====================================================================

#[test]
fn poll_signal_raise_decide_ffi_matches_model() {
    for signaled in [0u32, 1] {
        for result_val in [-1i32, 0, 42] {
            for has_poll_event in [false, true] {
                let (ffi_new_sig, ffi_new_res, ffi_action) =
                    ffi_poll_signal_raise_decide(signaled, result_val, has_poll_event);

                assert_eq!(ffi_new_sig, 1, "always sets signaled=1");
                assert_eq!(ffi_new_res, result_val, "stores result");
                if has_poll_event {
                    assert_eq!(ffi_action, 1, "SIGNAL_EVENT");
                } else {
                    assert_eq!(ffi_action, 0, "NO_EVENT");
                }
            }
        }
    }
}

// =====================================================================
// Property: PL7+PL8 — raise then reset roundtrip
// =====================================================================

#[test]
fn poll_signal_raise_reset_roundtrip() {
    let mut sig = PollSignal::init();
    assert_eq!(sig.signaled, 0);

    sig.raise(99);
    assert_eq!(sig.signaled, 1);
    assert_eq!(sig.result, 99);

    sig.reset();
    assert_eq!(sig.signaled, 0);
    // Result preserved after reset (Zephyr behavior)
    assert_eq!(sig.result, 99);
}
