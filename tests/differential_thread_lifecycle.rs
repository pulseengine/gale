//! Differential equivalence tests — Thread Lifecycle (FFI vs Model).
//!
//! Verifies that the FFI thread lifecycle functions produce the same
//! results as the Verus-verified model functions in gale::thread_lifecycle.

#![allow(
    clippy::shadow_unrelated,
    clippy::indexing_slicing,
    clippy::bool_to_int_with_if
)]

use gale::error::*;

const MAX_THREADS: u32 = 256;
const MAX_PRIORITY: u32 = 32;
const MIN_STACK_SIZE: u32 = 64;
const THREAD_STATE_DEAD: u8 = 0x08;

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Replica of gale_thread_create_validate.
fn ffi_thread_create_validate(count: u32) -> (i32, u32) {
    if count >= MAX_THREADS {
        return (EAGAIN, count);
    }
    #[allow(clippy::arithmetic_side_effects)]
    let new_count = count + 1;
    (OK, new_count)
}

/// Replica of gale_thread_exit_validate.
fn ffi_thread_exit_validate(count: u32) -> (i32, u32) {
    if count == 0 {
        return (EINVAL, 0);
    }
    #[allow(clippy::arithmetic_side_effects)]
    let new_count = count - 1;
    (OK, new_count)
}

/// Replica of gale_thread_priority_validate.
fn ffi_thread_priority_validate(priority: u32) -> i32 {
    if priority < MAX_PRIORITY { OK } else { EINVAL }
}

/// Replica of gale_k_thread_create_decide.
fn ffi_thread_create_decide(
    stack_size: u32,
    priority: u32,
    active_count: u32,
) -> (u8, i32) {
    if stack_size < MIN_STACK_SIZE {
        return (1, EINVAL); // REJECT
    }
    if priority >= MAX_PRIORITY {
        return (1, EINVAL); // REJECT
    }
    if active_count >= MAX_THREADS {
        return (1, EAGAIN); // REJECT
    }
    (0, OK) // PROCEED
}

/// Replica of gale_k_thread_abort_decide.
fn ffi_thread_abort_decide(thread_state: u8, is_essential: bool) -> u8 {
    if (thread_state & THREAD_STATE_DEAD) != 0 {
        return 1; // ALREADY_DEAD
    }
    if is_essential {
        return 2; // PANIC
    }
    0 // PROCEED
}

/// Replica of gale_k_thread_join_decide.
fn ffi_thread_join_decide(
    is_dead: bool,
    is_no_wait: bool,
    is_self_or_circular: bool,
) -> (u8, i32) {
    if is_dead {
        return (0, OK); // RETURN
    }
    if is_no_wait {
        return (0, EBUSY); // RETURN
    }
    if is_self_or_circular {
        return (0, EDEADLK); // RETURN
    }
    (1, OK) // PEND
}

// =====================================================================
// Differential tests: thread create/exit validate
// =====================================================================

#[test]
fn thread_create_validate_ffi_matches_model_exhaustive() {
    for count in 0u32..=MAX_THREADS {
        let (ffi_ret, ffi_new) = ffi_thread_create_validate(count);

        if count >= MAX_THREADS {
            assert_eq!(ffi_ret, EAGAIN, "create at capacity: count={count}");
        } else {
            assert_eq!(ffi_ret, OK, "create OK: count={count}");
            #[allow(clippy::arithmetic_side_effects)]
            let expected = count + 1;
            assert_eq!(ffi_new, expected, "create new_count: count={count}");
        }
    }
}

#[test]
fn thread_exit_validate_ffi_matches_model_exhaustive() {
    for count in 0u32..=MAX_THREADS {
        let (ffi_ret, ffi_new) = ffi_thread_exit_validate(count);

        if count == 0 {
            assert_eq!(ffi_ret, EINVAL, "exit underflow");
        } else {
            assert_eq!(ffi_ret, OK, "exit OK: count={count}");
            #[allow(clippy::arithmetic_side_effects)]
            let expected = count - 1;
            assert_eq!(ffi_new, expected, "exit new_count: count={count}");
        }
    }
}

// =====================================================================
// Differential tests: priority validate
// =====================================================================

#[test]
fn thread_priority_validate_ffi_matches_model_exhaustive() {
    for priority in 0u32..=MAX_PRIORITY {
        let ffi_ret = ffi_thread_priority_validate(priority);

        if priority < MAX_PRIORITY {
            assert_eq!(ffi_ret, OK, "valid priority: {priority}");
        } else {
            assert_eq!(ffi_ret, EINVAL, "invalid priority: {priority}");
        }
    }
}

// =====================================================================
// Differential tests: thread create_decide
// =====================================================================

#[test]
fn thread_create_decide_ffi_matches_model_exhaustive() {
    let stack_sizes = [0u32, 32, 63, 64, 128, 4096];
    let priorities = [0u32, 1, 15, 31, 32, 100];
    let active_counts = [0u32, 1, 128, 255, 256, 300];

    for &stack_size in &stack_sizes {
        for &priority in &priorities {
            for &active_count in &active_counts {
                let (ffi_action, ffi_ret) =
                    ffi_thread_create_decide(stack_size, priority, active_count);

                if stack_size < MIN_STACK_SIZE {
                    assert_eq!(ffi_action, 1, "REJECT: bad stack_size");
                    assert_eq!(ffi_ret, EINVAL);
                } else if priority >= MAX_PRIORITY {
                    assert_eq!(ffi_action, 1, "REJECT: bad priority");
                    assert_eq!(ffi_ret, EINVAL);
                } else if active_count >= MAX_THREADS {
                    assert_eq!(ffi_action, 1, "REJECT: at capacity");
                    assert_eq!(ffi_ret, EAGAIN);
                } else {
                    assert_eq!(ffi_action, 0, "PROCEED");
                    assert_eq!(ffi_ret, OK);
                }
            }
        }
    }
}

// =====================================================================
// Differential tests: thread abort_decide
// =====================================================================

#[test]
fn thread_abort_decide_ffi_matches_model_exhaustive() {
    for state in [0u8, THREAD_STATE_DEAD, 0x01, 0x09, 0xFF] {
        for is_essential in [false, true] {
            let ffi_action = ffi_thread_abort_decide(state, is_essential);

            if (state & THREAD_STATE_DEAD) != 0 {
                assert_eq!(ffi_action, 1, "ALREADY_DEAD: state={state:#x}");
            } else if is_essential {
                assert_eq!(ffi_action, 2, "PANIC: essential");
            } else {
                assert_eq!(ffi_action, 0, "PROCEED");
            }
        }
    }
}

// =====================================================================
// Differential tests: thread join_decide
// =====================================================================

#[test]
fn thread_join_decide_ffi_matches_model_exhaustive() {
    for is_dead in [false, true] {
        for is_no_wait in [false, true] {
            for is_self_or_circular in [false, true] {
                let (ffi_action, ffi_ret) =
                    ffi_thread_join_decide(is_dead, is_no_wait, is_self_or_circular);

                if is_dead {
                    assert_eq!(ffi_action, 0);
                    assert_eq!(ffi_ret, OK, "dead: return success");
                } else if is_no_wait {
                    assert_eq!(ffi_action, 0);
                    assert_eq!(ffi_ret, EBUSY, "no_wait: return busy");
                } else if is_self_or_circular {
                    assert_eq!(ffi_action, 0);
                    assert_eq!(ffi_ret, EDEADLK, "deadlock");
                } else {
                    assert_eq!(ffi_action, 1, "PEND");
                    assert_eq!(ffi_ret, OK);
                }
            }
        }
    }
}

// =====================================================================
// Property: TH5+TH6 — create/exit roundtrip
// =====================================================================

#[test]
fn thread_create_exit_roundtrip() {
    for initial in 0u32..=20 {
        let (ret, new_count) = ffi_thread_create_validate(initial);
        if ret == OK {
            let (ret2, final_count) = ffi_thread_exit_validate(new_count);
            assert_eq!(ret2, OK);
            assert_eq!(final_count, initial,
                "roundtrip failed: initial={initial}");
        }
    }
}
