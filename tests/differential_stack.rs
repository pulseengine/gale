//! Differential equivalence tests — Stack (FFI vs Model).
//!
//! Verifies that the FFI stack functions produce the same results as
//! the Verus-verified model functions in gale::stack.

#![allow(
    clippy::shadow_unrelated,
    clippy::indexing_slicing,
    clippy::bool_to_int_with_if,
    clippy::unwrap_used,
    clippy::fn_params_excessive_bools,
    clippy::absurd_extreme_comparisons,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::checked_conversions,
    clippy::wildcard_enum_match_arm,
    clippy::implicit_saturating_sub,
    clippy::branches_sharing_code,
    clippy::panic
)]

use gale::error::*;
use gale::stack::{self, PopDecision, PushDecision, Stack};

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Replica of gale_stack_init_validate.
fn ffi_stack_init_validate(num_entries: u32) -> i32 {
    if num_entries == 0 {
        EINVAL
    } else {
        OK
    }
}

/// Replica of gale_stack_push_validate.
fn ffi_stack_push_validate(count: u32, capacity: u32) -> (i32, u32) {
    use gale::stack::{push_decide, PushDecision};
    let r = push_decide(count, capacity, false);
    match r.decision {
        PushDecision::Store => (OK, r.new_count),
        PushDecision::Full => (ENOMEM, count),
        PushDecision::WakeWaiter => (OK, r.new_count),
    }
}

/// Replica of gale_stack_pop_validate.
fn ffi_stack_pop_validate(count: u32) -> (i32, u32) {
    use gale::stack::{pop_decide, PopDecision};
    let r = pop_decide(count, true); // is_no_wait=true: empty => EBUSY not Pend
    match r.decision {
        PopDecision::Pop => (OK, r.new_count),
        PopDecision::Busy => (EBUSY, count),
        PopDecision::Pend => (EBUSY, count),
    }
}

/// Replica of gale_k_stack_push_decide.
fn ffi_k_stack_push_decide(count: u32, capacity: u32, has_waiter: bool) -> (u8, u32, i32) {
    // action: 0=STORE, 1=WAKE, 2=FULL
    if has_waiter {
        (1, count, OK)          // WAKE_WAITER — count unchanged
    } else if count < capacity {
        #[allow(clippy::arithmetic_side_effects)]
        (0, count + 1, OK)      // STORE — count incremented
    } else {
        (2, count, ENOMEM)      // FULL — count unchanged
    }
}

/// Replica of gale_k_stack_pop_decide.
fn ffi_k_stack_pop_decide(count: u32, is_no_wait: bool) -> (u8, u32, i32) {
    // action: 0=POP_OK, 1=PEND_CURRENT
    if count > 0 {
        #[allow(clippy::arithmetic_side_effects)]
        (0, count - 1, OK)          // POP_OK
    } else if is_no_wait {
        (0, 0, EBUSY)               // POP_OK action but EBUSY return (Busy decision)
    } else {
        (1, 0, 0)                   // PEND_CURRENT
    }
}

// =====================================================================
// Differential tests: stack init_validate
// =====================================================================

#[test]
fn stack_init_validate_ffi_matches_model_exhaustive() {
    for num_entries in 0u32..=20 {
        let ffi_ret = ffi_stack_init_validate(num_entries);

        let model_result = Stack::init(num_entries);

        if num_entries == 0 {
            assert_eq!(ffi_ret, EINVAL,
                "init: zero entries should be EINVAL");
            assert!(model_result.is_err(),
                "model init: zero entries should fail");
        } else {
            assert_eq!(ffi_ret, OK,
                "init: nonzero entries should be OK");
            let s = model_result.unwrap();
            assert_eq!(s.capacity, num_entries);
            assert_eq!(s.count, 0);
        }
    }
}

// =====================================================================
// Differential tests: stack push_validate
// =====================================================================

#[test]
fn stack_push_validate_ffi_matches_model_exhaustive() {
    for capacity in 1u32..=10 {
        for count in 0u32..=capacity {
            let (ffi_ret, ffi_new) = ffi_stack_push_validate(count, capacity);

            let mut s = Stack { capacity, count };
            let model_ret = s.push();

            assert_eq!(ffi_ret, model_ret,
                "push ret: capacity={capacity}, count={count}");
            if ffi_ret == OK {
                assert_eq!(ffi_new, s.count,
                    "push new_count: capacity={capacity}, count={count}");
            }
        }
    }
}

#[test]
fn stack_push_validate_full_returns_enomem() {
    for capacity in 1u32..=10 {
        let (ret, _) = ffi_stack_push_validate(capacity, capacity);
        assert_eq!(ret, ENOMEM,
            "push on full stack: capacity={capacity}");
    }
}

// =====================================================================
// Differential tests: stack pop_validate
// =====================================================================

#[test]
fn stack_pop_validate_ffi_matches_model_exhaustive() {
    for capacity in 1u32..=10 {
        for count in 0u32..=capacity {
            let (ffi_ret, ffi_new) = ffi_stack_pop_validate(count);

            let mut s = Stack { capacity, count };
            let model_ret = s.pop();

            assert_eq!(ffi_ret, model_ret,
                "pop ret: capacity={capacity}, count={count}");
            if ffi_ret == OK {
                assert_eq!(ffi_new, s.count,
                    "pop new_count: capacity={capacity}, count={count}");
            }
        }
    }
}

#[test]
fn stack_pop_validate_empty_returns_ebusy() {
    let (ret, _) = ffi_stack_pop_validate(0);
    assert_eq!(ret, EBUSY, "pop on empty stack should be EBUSY");
}

// =====================================================================
// Differential tests: push_decide
// =====================================================================

#[test]
fn stack_push_decide_ffi_matches_model_exhaustive() {
    for capacity in 1u32..=8 {
        for count in 0u32..=capacity {
            for has_waiter in [false, true] {
                let (ffi_action, ffi_new, ffi_ret) =
                    ffi_k_stack_push_decide(count, capacity, has_waiter);

                let model = stack::push_decide(count, capacity, has_waiter);

                let (expected_action, expected_new, expected_ret) = match model.decision {
                    PushDecision::Store => (0u8, model.new_count, OK),
                    PushDecision::WakeWaiter => (1u8, model.new_count, OK),
                    PushDecision::Full => (2u8, model.new_count, ENOMEM),
                };

                assert_eq!(ffi_action, expected_action,
                    "push_decide action: cap={capacity}, cnt={count}, waiter={has_waiter}");
                assert_eq!(ffi_new, expected_new,
                    "push_decide new_count: cap={capacity}, cnt={count}, waiter={has_waiter}");
                assert_eq!(ffi_ret, expected_ret,
                    "push_decide ret: cap={capacity}, cnt={count}, waiter={has_waiter}");
            }
        }
    }
}

// =====================================================================
// Differential tests: pop_decide
// =====================================================================

#[test]
fn stack_pop_decide_ffi_matches_model_exhaustive() {
    for count in 0u32..=10 {
        for is_no_wait in [false, true] {
            let (ffi_action, ffi_new, ffi_ret) =
                ffi_k_stack_pop_decide(count, is_no_wait);

            let model = stack::pop_decide(count, is_no_wait);

            match model.decision {
                PopDecision::Pop => {
                    assert_eq!(ffi_action, 0, "pop_decide: Pop action: count={count}");
                    assert_eq!(ffi_ret, OK, "pop_decide: Pop ret: count={count}");
                    assert_eq!(ffi_new, model.new_count,
                        "pop_decide: Pop new_count: count={count}");
                }
                PopDecision::Busy => {
                    assert_eq!(ffi_action, 0, "pop_decide: Busy action: count={count}");
                    assert_eq!(ffi_ret, EBUSY, "pop_decide: Busy ret: count={count}");
                    assert_eq!(ffi_new, 0, "pop_decide: Busy new_count: count={count}");
                }
                PopDecision::Pend => {
                    assert_eq!(ffi_action, 1, "pop_decide: Pend action: count={count}");
                    assert_eq!(ffi_ret, 0, "pop_decide: Pend ret: count={count}");
                    assert_eq!(ffi_new, 0, "pop_decide: Pend new_count: count={count}");
                }
            }
        }
    }
}

// =====================================================================
// Property: SK3 — waiter always wakes (never store or full)
// =====================================================================

#[test]
fn stack_push_decide_waiter_always_wakes() {
    for capacity in 1u32..=8 {
        for count in 0u32..=capacity {
            let (action, _, ret) = ffi_k_stack_push_decide(count, capacity, true);
            assert_eq!(action, 1u8, "SK3: waiter must wake: cap={capacity}, cnt={count}");
            assert_eq!(ret, OK, "SK3: waiter wake must return OK");
        }
    }
}

// =====================================================================
// Property: SK4 — full stack rejects push (no waiter)
// =====================================================================

#[test]
fn stack_push_decide_full_rejects() {
    for capacity in 1u32..=8 {
        let (action, new_count, ret) = ffi_k_stack_push_decide(capacity, capacity, false);
        assert_eq!(action, 2u8, "SK4: full stack action must be FULL: cap={capacity}");
        assert_eq!(ret, ENOMEM, "SK4: full stack must return ENOMEM: cap={capacity}");
        assert_eq!(new_count, capacity, "SK4: full stack count unchanged: cap={capacity}");
    }
}

// =====================================================================
// Property: SK5/SK6 — pop: non-empty decrements, empty returns EBUSY
// =====================================================================

#[test]
fn stack_pop_decide_nonempty_decrements() {
    for count in 1u32..=10 {
        let (action, new_count, ret) = ffi_k_stack_pop_decide(count, true);
        assert_eq!(action, 0u8, "SK5: pop action must be POP_OK: count={count}");
        assert_eq!(ret, OK, "SK5: pop must return OK: count={count}");
        #[allow(clippy::arithmetic_side_effects)]
        let expected = count - 1;
        assert_eq!(new_count, expected, "SK5: pop must decrement count: count={count}");
    }
}

// =====================================================================
// Property: SK9 — push-pop roundtrip preserves count
// =====================================================================

#[test]
fn stack_push_pop_roundtrip() {
    for capacity in 1u32..=10 {
        for initial in 0u32..capacity {
            let mut s = Stack { capacity, count: initial };

            let push_ret = s.push();
            assert_eq!(push_ret, OK, "push should succeed: cap={capacity}, cnt={initial}");

            let pop_ret = s.pop();
            assert_eq!(pop_ret, OK, "pop should succeed after push");
            assert_eq!(s.count, initial, "SK9: roundtrip must preserve count");
        }
    }
}
