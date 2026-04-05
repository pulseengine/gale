//! Differential equivalence tests — Semaphore (FFI vs Model).
//!
//! Verifies that the FFI semaphore functions produce the same results as
//! the Verus-verified model functions in gale::sem.

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
use gale::sem::{self, GiveDecision, TakeDecision};

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Replica of gale_sem_count_init (ffi/src/lib.rs).
fn ffi_sem_count_init(initial_count: u32, limit: u32) -> i32 {
    if limit == 0 || initial_count > limit {
        EINVAL
    } else {
        OK
    }
}

/// Replica of gale_sem_count_give (ffi/src/lib.rs).
fn ffi_sem_count_give(count: u32, limit: u32) -> u32 {
    // Mirrors: count += (count != limit) ? 1 : 0
    if count != limit {
        #[allow(clippy::arithmetic_side_effects)]
        let new_count = count + 1;
        new_count
    } else {
        count
    }
}

/// Decision struct matching GaleSemGiveDecision.
#[derive(Debug, PartialEq, Eq)]
struct FfiSemGiveDecision {
    action: u8,
    new_count: u32,
}

const FFI_SEM_ACTION_INCREMENT: u8 = 0;
const FFI_SEM_ACTION_WAKE: u8 = 1;

/// Replica of gale_k_sem_give_decide (ffi/src/lib.rs).
fn ffi_sem_give_decide(count: u32, limit: u32, has_waiter: bool) -> FfiSemGiveDecision {
    if has_waiter {
        FfiSemGiveDecision {
            action: FFI_SEM_ACTION_WAKE,
            new_count: count,
        }
    } else if count < limit {
        #[allow(clippy::arithmetic_side_effects)]
        let new_count = count + 1;
        FfiSemGiveDecision {
            action: FFI_SEM_ACTION_INCREMENT,
            new_count,
        }
    } else {
        // Saturated: action=INCREMENT but count unchanged
        FfiSemGiveDecision {
            action: FFI_SEM_ACTION_INCREMENT,
            new_count: count,
        }
    }
}

/// Decision struct matching GaleSemTakeDecision.
#[derive(Debug, PartialEq, Eq)]
struct FfiSemTakeDecision {
    ret: i32,
    new_count: u32,
    action: u8,
}

const FFI_SEM_TAKE_RETURN: u8 = 0;
const FFI_SEM_TAKE_PEND: u8 = 1;

/// Replica of gale_k_sem_take_decide (ffi/src/lib.rs).
fn ffi_sem_take_decide(count: u32, is_no_wait: bool) -> FfiSemTakeDecision {
    if count > 0 {
        #[allow(clippy::arithmetic_side_effects)]
        let new_count = count - 1;
        FfiSemTakeDecision {
            ret: OK,
            new_count,
            action: FFI_SEM_TAKE_RETURN,
        }
    } else if is_no_wait {
        FfiSemTakeDecision {
            ret: EBUSY,
            new_count: 0,
            action: FFI_SEM_TAKE_RETURN,
        }
    } else {
        FfiSemTakeDecision {
            ret: 0,
            new_count: 0,
            action: FFI_SEM_TAKE_PEND,
        }
    }
}

// =====================================================================
// Differential tests: sem init validate
// =====================================================================

#[test]
fn sem_init_validate_ffi_matches_model_exhaustive() {
    for initial_count in 0u32..=10 {
        for limit in 0u32..=10 {
            let ffi_ret = ffi_sem_count_init(initial_count, limit);

            // Model validation: limit==0 or initial_count>limit -> EINVAL
            let model_ret = if limit == 0 || initial_count > limit {
                EINVAL
            } else {
                OK
            };

            assert_eq!(
                ffi_ret, model_ret,
                "init_validate mismatch: initial={initial_count}, limit={limit}"
            );
        }
    }
}

#[test]
fn sem_init_validate_boundary_conditions() {
    // limit=0 always EINVAL
    assert_eq!(ffi_sem_count_init(0, 0), EINVAL);
    assert_eq!(ffi_sem_count_init(1, 0), EINVAL);

    // initial > limit always EINVAL
    assert_eq!(ffi_sem_count_init(5, 4), EINVAL);
    assert_eq!(ffi_sem_count_init(u32::MAX, 1), EINVAL);

    // valid: initial == limit
    assert_eq!(ffi_sem_count_init(5, 5), OK);

    // valid: initial == 0
    assert_eq!(ffi_sem_count_init(0, 1), OK);
    assert_eq!(ffi_sem_count_init(0, u32::MAX), OK);
}

// =====================================================================
// Differential tests: sem give_decide
// =====================================================================

#[test]
fn sem_give_decide_ffi_matches_model_exhaustive() {
    // Limit must be >0 (model invariant)
    for limit in 1u32..=8 {
        for count in 0u32..=limit {
            for has_waiter in [false, true] {
                let ffi_d = ffi_sem_give_decide(count, limit, has_waiter);
                let model_d = sem::give_decide(count, limit, has_waiter);

                match model_d {
                    GiveDecision::WakeThread => {
                        assert_eq!(
                            ffi_d.action, FFI_SEM_ACTION_WAKE,
                            "give_decide action mismatch: count={count}, limit={limit}, \
                             has_waiter={has_waiter}"
                        );
                        assert_eq!(
                            ffi_d.new_count, count,
                            "give_decide WakeThread: new_count should equal count"
                        );
                    }
                    GiveDecision::Increment => {
                        assert_eq!(ffi_d.action, FFI_SEM_ACTION_INCREMENT);
                        #[allow(clippy::arithmetic_side_effects)]
                        let expected_new = count + 1;
                        assert_eq!(
                            ffi_d.new_count, expected_new,
                            "give_decide Increment: count={count}, limit={limit}"
                        );
                    }
                    GiveDecision::Saturated => {
                        assert_eq!(ffi_d.action, FFI_SEM_ACTION_INCREMENT);
                        assert_eq!(
                            ffi_d.new_count, count,
                            "give_decide Saturated: count should be unchanged"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn sem_give_decide_waiter_takes_priority() {
    // has_waiter=true always => WakeThread regardless of count/limit
    for limit in 1u32..=5 {
        for count in 0u32..=limit {
            let ffi_d = ffi_sem_give_decide(count, limit, true);
            let model_d = sem::give_decide(count, limit, true);

            assert_eq!(model_d, GiveDecision::WakeThread);
            assert_eq!(ffi_d.action, FFI_SEM_ACTION_WAKE);
            assert_eq!(ffi_d.new_count, count);
        }
    }
}

#[test]
fn sem_give_decide_saturation_no_overflow() {
    // count == limit, no waiter: should saturate (count unchanged)
    for limit in 1u32..=10 {
        let ffi_d = ffi_sem_give_decide(limit, limit, false);
        let model_d = sem::give_decide(limit, limit, false);

        assert_eq!(model_d, GiveDecision::Saturated);
        assert_eq!(ffi_d.action, FFI_SEM_ACTION_INCREMENT);
        assert_eq!(ffi_d.new_count, limit, "saturation: new_count must not exceed limit");
    }
}

// =====================================================================
// Differential tests: sem count_give (legacy path)
// =====================================================================

#[test]
fn sem_count_give_ffi_matches_model_exhaustive() {
    for limit in 1u32..=8 {
        for count in 0u32..=limit {
            let ffi_new = ffi_sem_count_give(count, limit);
            // Model: give_decide with has_waiter=false, then apply
            let model_d = sem::give_decide(count, limit, false);
            let model_new = match model_d {
                GiveDecision::Increment => {
                    #[allow(clippy::arithmetic_side_effects)]
                    let n = count + 1;
                    n
                }
                GiveDecision::Saturated | GiveDecision::WakeThread => count,
            };

            assert_eq!(
                ffi_new, model_new,
                "count_give mismatch: count={count}, limit={limit}"
            );
        }
    }
}

// =====================================================================
// Differential tests: sem take_decide
// =====================================================================

#[test]
fn sem_take_decide_ffi_matches_model_exhaustive() {
    for count in 0u32..=10 {
        for is_no_wait in [false, true] {
            let ffi_d = ffi_sem_take_decide(count, is_no_wait);
            let model_d = sem::take_decide(count, is_no_wait);

            match model_d {
                TakeDecision::Acquired => {
                    assert_eq!(ffi_d.ret, OK, "Acquired: ret should be OK");
                    assert_eq!(ffi_d.action, FFI_SEM_TAKE_RETURN);
                    #[allow(clippy::arithmetic_side_effects)]
                    let expected_new = count - 1;
                    assert_eq!(
                        ffi_d.new_count, expected_new,
                        "Acquired: new_count should decrement"
                    );
                }
                TakeDecision::WouldBlock => {
                    assert_eq!(ffi_d.ret, EBUSY, "WouldBlock: ret should be EBUSY");
                    assert_eq!(ffi_d.action, FFI_SEM_TAKE_RETURN);
                }
                TakeDecision::Pend => {
                    assert_eq!(ffi_d.ret, 0, "Pend: ret should be 0");
                    assert_eq!(ffi_d.action, FFI_SEM_TAKE_PEND);
                }
            }
        }
    }
}

#[test]
fn sem_take_decide_count_zero_no_wait_returns_ebusy() {
    let ffi_d = ffi_sem_take_decide(0, true);
    let model_d = sem::take_decide(0, true);
    assert_eq!(model_d, TakeDecision::WouldBlock);
    assert_eq!(ffi_d.ret, EBUSY);
    assert_eq!(ffi_d.action, FFI_SEM_TAKE_RETURN);
}

#[test]
fn sem_take_decide_count_zero_wait_pends() {
    let ffi_d = ffi_sem_take_decide(0, false);
    let model_d = sem::take_decide(0, false);
    assert_eq!(model_d, TakeDecision::Pend);
    assert_eq!(ffi_d.ret, 0);
    assert_eq!(ffi_d.action, FFI_SEM_TAKE_PEND);
}

// =====================================================================
// Property: P3 — give then take roundtrip
// =====================================================================

#[test]
fn sem_give_take_roundtrip() {
    for limit in 1u32..=8 {
        for count in 0u32..limit {
            // give: count -> count+1
            let give_d = ffi_sem_give_decide(count, limit, false);
            assert_eq!(give_d.action, FFI_SEM_ACTION_INCREMENT);
            let after_give = give_d.new_count;

            // take: after_give -> after_give-1 == count
            let take_d = ffi_sem_take_decide(after_give, true);
            assert_eq!(take_d.ret, OK);
            assert_eq!(
                take_d.new_count, count,
                "roundtrip failed: count={count}, limit={limit}"
            );
        }
    }
}

// =====================================================================
// Property: P9 — no arithmetic overflow (saturation at limit)
// =====================================================================

#[test]
fn sem_give_no_overflow_at_max_limit() {
    let limit = u32::MAX;
    // count == limit: saturation, no overflow
    let ffi_d = ffi_sem_give_decide(limit, limit, false);
    assert_eq!(ffi_d.new_count, limit, "must not overflow u32");
}
