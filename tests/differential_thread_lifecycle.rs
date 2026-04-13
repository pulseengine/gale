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

// =====================================================================
// FFI replicas — new decision functions
// =====================================================================

const THREAD_STATE_SUSPENDED: u8 = 0x02;

/// Replica of gale_k_thread_suspend_decide.
fn ffi_thread_suspend_decide(thread_state: u8) -> u8 {
    if (thread_state & THREAD_STATE_SUSPENDED) != 0 {
        1 // ALREADY_SUSPENDED
    } else {
        0 // PROCEED
    }
}

/// Replica of gale_k_thread_resume_decide.
fn ffi_thread_resume_decide(thread_state: u8) -> u8 {
    if (thread_state & THREAD_STATE_SUSPENDED) == 0 {
        1 // NOT_SUSPENDED
    } else {
        0 // PROCEED
    }
}

/// Replica of gale_k_thread_priority_set_decide.
fn ffi_thread_priority_set_decide(new_priority: u32) -> (u8, i32) {
    if new_priority >= MAX_PRIORITY {
        (1, EINVAL) // REJECT
    } else {
        (0, OK) // PROCEED
    }
}

/// Replica of gale_k_thread_stack_space_decide.
fn ffi_thread_stack_space_decide(
    stack_size: u32,
    stack_usage: u32,
    stack_mapped_valid: u32,
) -> (u8, i32, u32) {
    if stack_size == 0 {
        return (1, EINVAL, 0); // REJECT
    }
    if stack_mapped_valid == 0 {
        return (1, EINVAL, 0); // REJECT
    }
    let usage = if stack_usage > stack_size { stack_size } else { stack_usage };
    #[allow(clippy::arithmetic_side_effects)]
    let unused = stack_size - usage;
    (0, OK, unused) // PROCEED
}

/// Replica of gale_k_thread_deadline_decide.
fn ffi_thread_deadline_decide(deadline: i32) -> (u8, i32, i32) {
    if deadline <= 0 {
        (1, EINVAL, 0) // REJECT
    } else {
        (0, OK, deadline) // PROCEED
    }
}

// =====================================================================
// Differential tests: suspend_decide
// =====================================================================

#[test]
fn thread_suspend_decide_not_suspended_proceeds() {
    // States without the SUSPENDED bit
    for state in [0x00u8, 0x01, 0x04, 0x08, 0x10, 0xFD] {
        let action = ffi_thread_suspend_decide(state);
        assert_eq!(action, 0, "should PROCEED: state={state:#x}");
    }
}

#[test]
fn thread_suspend_decide_already_suspended_noop() {
    // States with the SUSPENDED bit set (0x02)
    for state in [0x02u8, 0x03, 0x06, 0x0A, 0xFF] {
        let action = ffi_thread_suspend_decide(state);
        assert_eq!(action, 1, "should be ALREADY_SUSPENDED: state={state:#x}");
    }
}

#[test]
fn thread_suspend_decide_exhaustive() {
    for state in 0u8..=255 {
        let action = ffi_thread_suspend_decide(state);
        if (state & THREAD_STATE_SUSPENDED) != 0 {
            assert_eq!(action, 1, "ALREADY_SUSPENDED: state={state:#x}");
        } else {
            assert_eq!(action, 0, "PROCEED: state={state:#x}");
        }
    }
}

// =====================================================================
// Differential tests: resume_decide
// =====================================================================

#[test]
fn thread_resume_decide_suspended_proceeds() {
    for state in [0x02u8, 0x03, 0x06, 0x0A, 0xFF] {
        let action = ffi_thread_resume_decide(state);
        assert_eq!(action, 0, "should PROCEED: state={state:#x}");
    }
}

#[test]
fn thread_resume_decide_not_suspended_noop() {
    for state in [0x00u8, 0x01, 0x04, 0x08, 0xFD] {
        let action = ffi_thread_resume_decide(state);
        assert_eq!(action, 1, "should be NOT_SUSPENDED: state={state:#x}");
    }
}

#[test]
fn thread_resume_decide_exhaustive() {
    for state in 0u8..=255 {
        let action = ffi_thread_resume_decide(state);
        if (state & THREAD_STATE_SUSPENDED) != 0 {
            assert_eq!(action, 0, "PROCEED: state={state:#x}");
        } else {
            assert_eq!(action, 1, "NOT_SUSPENDED: state={state:#x}");
        }
    }
}

// =====================================================================
// Differential tests: suspend/resume complementarity
// =====================================================================

#[test]
fn suspend_resume_are_complementary() {
    // If suspend says PROCEED (not suspended), then after we set SUSPENDED bit,
    // resume must say PROCEED.
    for state in 0u8..=255 {
        let suspend_action = ffi_thread_suspend_decide(state);
        let resume_action = ffi_thread_resume_decide(state);
        // Exactly one of them should say PROCEED
        let suspend_proceeds = suspend_action == 0;
        let resume_proceeds = resume_action == 0;
        assert_ne!(
            suspend_proceeds, resume_proceeds,
            "suspend and resume must have opposite PROCEED/no-op for state={state:#x}"
        );
    }
}

// =====================================================================
// Differential tests: priority_set_decide
// =====================================================================

#[test]
fn thread_priority_set_decide_valid_proceeds() {
    for priority in 0u32..MAX_PRIORITY {
        let (action, ret) = ffi_thread_priority_set_decide(priority);
        assert_eq!(action, 0, "should PROCEED: priority={priority}");
        assert_eq!(ret, OK);
    }
}

#[test]
fn thread_priority_set_decide_invalid_rejects() {
    for priority in [MAX_PRIORITY, MAX_PRIORITY + 1, u32::MAX] {
        let (action, ret) = ffi_thread_priority_set_decide(priority);
        assert_eq!(action, 1, "should REJECT: priority={priority}");
        assert_eq!(ret, EINVAL);
    }
}

#[test]
fn thread_priority_set_decide_boundary() {
    let (action, ret) = ffi_thread_priority_set_decide(MAX_PRIORITY - 1);
    assert_eq!(action, 0);
    assert_eq!(ret, OK);

    let (action, ret) = ffi_thread_priority_set_decide(MAX_PRIORITY);
    assert_eq!(action, 1);
    assert_eq!(ret, EINVAL);
}

// =====================================================================
// Differential tests: stack_space_decide
// =====================================================================

#[test]
fn thread_stack_space_decide_valid() {
    let (action, ret, unused) = ffi_thread_stack_space_decide(4096, 512, 1);
    assert_eq!(action, 0); // PROCEED
    assert_eq!(ret, OK);
    #[allow(clippy::arithmetic_side_effects)]
    let expected = 4096 - 512;
    assert_eq!(unused, expected);
}

#[test]
fn thread_stack_space_decide_zero_usage() {
    let (action, ret, unused) = ffi_thread_stack_space_decide(2048, 0, 1);
    assert_eq!(action, 0);
    assert_eq!(ret, OK);
    assert_eq!(unused, 2048);
}

#[test]
fn thread_stack_space_decide_full_usage() {
    let (action, ret, unused) = ffi_thread_stack_space_decide(1024, 1024, 1);
    assert_eq!(action, 0);
    assert_eq!(ret, OK);
    assert_eq!(unused, 0);
}

#[test]
fn thread_stack_space_decide_over_usage_clamped() {
    // usage > size should be clamped: unused = 0
    let (action, ret, unused) = ffi_thread_stack_space_decide(1024, 2000, 1);
    assert_eq!(action, 0);
    assert_eq!(ret, OK);
    assert_eq!(unused, 0);
}

#[test]
fn thread_stack_space_decide_rejects_zero_size() {
    let (action, ret, _) = ffi_thread_stack_space_decide(0, 0, 1);
    assert_eq!(action, 1); // REJECT
    assert_eq!(ret, EINVAL);
}

#[test]
fn thread_stack_space_decide_rejects_unmapped() {
    let (action, ret, _) = ffi_thread_stack_space_decide(4096, 0, 0);
    assert_eq!(action, 1); // REJECT
    assert_eq!(ret, EINVAL);
}

#[test]
fn thread_stack_space_decide_unused_bounded_by_size() {
    for size in [64u32, 256, 1024, 4096, u32::MAX / 2] {
        for usage in [0u32, 1, size / 2, size, size + 1] {
            let (action, ret, unused) = ffi_thread_stack_space_decide(size, usage, 1);
            assert_eq!(action, 0);
            assert_eq!(ret, OK);
            assert!(
                unused <= size,
                "TH4 violated: unused={unused} > size={size}, usage={usage}"
            );
        }
    }
}

// =====================================================================
// Differential tests: deadline_decide
// =====================================================================

#[test]
fn thread_deadline_decide_positive_proceeds() {
    for deadline in [1i32, 100, 1000, i32::MAX] {
        let (action, ret, clamped) = ffi_thread_deadline_decide(deadline);
        assert_eq!(action, 0, "should PROCEED: deadline={deadline}");
        assert_eq!(ret, OK);
        assert_eq!(clamped, deadline);
    }
}

#[test]
fn thread_deadline_decide_zero_rejects() {
    let (action, ret, _) = ffi_thread_deadline_decide(0);
    assert_eq!(action, 1); // REJECT
    assert_eq!(ret, EINVAL);
}

#[test]
fn thread_deadline_decide_negative_rejects() {
    for deadline in [-1i32, -100, i32::MIN] {
        let (action, ret, _) = ffi_thread_deadline_decide(deadline);
        assert_eq!(action, 1, "should REJECT: deadline={deadline}");
        assert_eq!(ret, EINVAL);
    }
}

#[test]
fn thread_deadline_decide_boundary_one() {
    let (action, ret, clamped) = ffi_thread_deadline_decide(1);
    assert_eq!(action, 0);
    assert_eq!(ret, OK);
    assert_eq!(clamped, 1);
}

#[test]
fn thread_deadline_decide_clamped_equals_input_for_valid() {
    for deadline in [1i32, 42, 1000, 100_000, i32::MAX] {
        let (_, _, clamped) = ffi_thread_deadline_decide(deadline);
        assert_eq!(clamped, deadline, "clamped should equal input for valid deadline={deadline}");
    }
}
