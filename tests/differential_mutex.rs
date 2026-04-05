//! Differential equivalence tests — Mutex (FFI vs Model).
//!
//! Verifies that the FFI mutex functions produce the same results as
//! the Verus-verified model functions in gale::mutex.

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
use gale::mutex::{self, LockDecision, UnlockDecisionKind};

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Decision struct matching GaleMutexLockDecision.
#[derive(Debug, PartialEq, Eq)]
struct FfiMutexLockDecision {
    ret: i32,
    action: u8,
    new_lock_count: u32,
}

const FFI_MUTEX_ACTION_ACQUIRED: u8 = 0;
const FFI_MUTEX_ACTION_PEND: u8 = 1;
const FFI_MUTEX_ACTION_BUSY: u8 = 2;

/// Replica of gale_k_mutex_lock_decide (ffi/src/lib.rs).
fn ffi_mutex_lock_decide(
    lock_count: u32,
    owner_is_null: bool,
    owner_is_current: bool,
    is_no_wait: bool,
) -> FfiMutexLockDecision {
    if owner_is_null {
        // Acquire: unlocked mutex
        FfiMutexLockDecision {
            ret: OK,
            action: FFI_MUTEX_ACTION_ACQUIRED,
            new_lock_count: 1,
        }
    } else if owner_is_current {
        // Reentrant or overflow
        if lock_count < u32::MAX {
            #[allow(clippy::arithmetic_side_effects)]
            let n = lock_count + 1;
            FfiMutexLockDecision {
                ret: OK,
                action: FFI_MUTEX_ACTION_ACQUIRED,
                new_lock_count: n,
            }
        } else {
            // Overflow
            FfiMutexLockDecision {
                ret: EINVAL,
                action: FFI_MUTEX_ACTION_BUSY,
                new_lock_count: lock_count,
            }
        }
    } else if is_no_wait {
        // Busy: different owner, no wait
        FfiMutexLockDecision {
            ret: EBUSY,
            action: FFI_MUTEX_ACTION_BUSY,
            new_lock_count: lock_count,
        }
    } else {
        // Pend: different owner, willing to wait
        FfiMutexLockDecision {
            ret: 0,
            action: FFI_MUTEX_ACTION_PEND,
            new_lock_count: lock_count,
        }
    }
}

/// Replica of gale_mutex_lock_validate (ffi/src/lib.rs).
/// Returns (ret, new_lock_count).
fn ffi_mutex_lock_validate(
    lock_count: u32,
    owner_is_null: bool,
    owner_is_current: bool,
) -> (i32, u32) {
    // lock_validate always uses is_no_wait=true
    let d = ffi_mutex_lock_decide(lock_count, owner_is_null, owner_is_current, true);
    match d.action {
        FFI_MUTEX_ACTION_ACQUIRED => (OK, d.new_lock_count),
        _ => (d.ret, lock_count),
    }
}

/// Decision struct matching GaleMutexUnlockDecision.
#[derive(Debug, PartialEq, Eq)]
struct FfiMutexUnlockDecision {
    ret: i32,
    action: u8,
    new_lock_count: u32,
}

const FFI_MUTEX_UNLOCK_RELEASED: u8 = 0;
const FFI_MUTEX_UNLOCK_UNLOCKED: u8 = 1;
const FFI_MUTEX_UNLOCK_ERROR: u8 = 2;

/// Replica of gale_k_mutex_unlock_decide (ffi/src/lib.rs).
fn ffi_mutex_unlock_decide(
    lock_count: u32,
    owner_is_null: bool,
    owner_is_current: bool,
) -> FfiMutexUnlockDecision {
    if owner_is_null {
        FfiMutexUnlockDecision {
            ret: EINVAL,
            action: FFI_MUTEX_UNLOCK_ERROR,
            new_lock_count: 0,
        }
    } else if !owner_is_current {
        FfiMutexUnlockDecision {
            ret: EPERM,
            action: FFI_MUTEX_UNLOCK_ERROR,
            new_lock_count: lock_count,
        }
    } else if lock_count > 1 {
        #[allow(clippy::arithmetic_side_effects)]
        let new_count = lock_count - 1;
        FfiMutexUnlockDecision {
            ret: OK,
            action: FFI_MUTEX_UNLOCK_RELEASED,
            new_lock_count: new_count,
        }
    } else {
        FfiMutexUnlockDecision {
            ret: OK,
            action: FFI_MUTEX_UNLOCK_UNLOCKED,
            new_lock_count: 0,
        }
    }
}

/// Replica of gale_mutex_unlock_validate (ffi/src/lib.rs).
/// Returns (ret, new_lock_count).
/// GALE_MUTEX_RELEASED=1, GALE_MUTEX_UNLOCKED=0.
fn ffi_mutex_unlock_validate(
    lock_count: u32,
    owner_is_null: bool,
    owner_is_current: bool,
) -> (i32, u32) {
    let d = ffi_mutex_unlock_decide(lock_count, owner_is_null, owner_is_current);
    match d.action {
        FFI_MUTEX_UNLOCK_RELEASED => (1, d.new_lock_count), // GALE_MUTEX_RELEASED
        FFI_MUTEX_UNLOCK_UNLOCKED => (0, 0),                // GALE_MUTEX_UNLOCKED
        _ => (d.ret, lock_count),
    }
}

// =====================================================================
// Differential tests: mutex lock_decide
// =====================================================================

#[test]
fn mutex_lock_decide_ffi_matches_model_exhaustive() {
    // Test lock_counts 0..5 plus u32::MAX for overflow
    let lock_counts: &[u32] = &[0, 1, 2, 3, 5, u32::MAX - 1, u32::MAX];
    for &lock_count in lock_counts {
        for owner_is_null in [false, true] {
            for owner_is_current in [false, true] {
                // owner_is_null and owner_is_current can't both be true
                // (undefined, skip)
                if owner_is_null && owner_is_current {
                    continue;
                }
                for is_no_wait in [false, true] {
                    let ffi_d = ffi_mutex_lock_decide(
                        lock_count,
                        owner_is_null,
                        owner_is_current,
                        is_no_wait,
                    );
                    let model_d = mutex::lock_decide(
                        lock_count,
                        owner_is_null,
                        owner_is_current,
                        is_no_wait,
                    );

                    match model_d {
                        LockDecision::Acquire => {
                            assert_eq!(ffi_d.ret, OK, "Acquire: ret");
                            assert_eq!(ffi_d.action, FFI_MUTEX_ACTION_ACQUIRED, "Acquire: action");
                            assert_eq!(ffi_d.new_lock_count, 1, "Acquire: new_lock_count");
                        }
                        LockDecision::Reentrant => {
                            assert_eq!(ffi_d.ret, OK, "Reentrant: ret");
                            assert_eq!(
                                ffi_d.action, FFI_MUTEX_ACTION_ACQUIRED,
                                "Reentrant: action"
                            );
                            #[allow(clippy::arithmetic_side_effects)]
                            let expected = lock_count + 1;
                            assert_eq!(
                                ffi_d.new_lock_count, expected,
                                "Reentrant: new_lock_count"
                            );
                        }
                        LockDecision::Overflow => {
                            assert_eq!(ffi_d.ret, EINVAL, "Overflow: ret");
                            assert_eq!(ffi_d.action, FFI_MUTEX_ACTION_BUSY, "Overflow: action");
                            assert_eq!(
                                ffi_d.new_lock_count, lock_count,
                                "Overflow: lock_count unchanged"
                            );
                        }
                        LockDecision::Busy => {
                            assert_eq!(ffi_d.ret, EBUSY, "Busy: ret");
                            assert_eq!(ffi_d.action, FFI_MUTEX_ACTION_BUSY, "Busy: action");
                        }
                        LockDecision::Pend => {
                            assert_eq!(ffi_d.ret, 0, "Pend: ret");
                            assert_eq!(ffi_d.action, FFI_MUTEX_ACTION_PEND, "Pend: action");
                        }
                    }
                }
            }
        }
    }
}

// =====================================================================
// Differential tests: mutex lock_validate (legacy API)
// =====================================================================

#[test]
fn mutex_lock_validate_ffi_matches_model_exhaustive() {
    let lock_counts: &[u32] = &[0, 1, 2, 5, u32::MAX - 1, u32::MAX];
    for &lock_count in lock_counts {
        for owner_is_null in [false, true] {
            for owner_is_current in [false, true] {
                if owner_is_null && owner_is_current {
                    continue;
                }

                let (ffi_ret, ffi_new) =
                    ffi_mutex_lock_validate(lock_count, owner_is_null, owner_is_current);
                // lock_validate uses is_no_wait=true
                let model_d =
                    mutex::lock_decide(lock_count, owner_is_null, owner_is_current, true);

                match model_d {
                    LockDecision::Acquire => {
                        assert_eq!(ffi_ret, OK);
                        assert_eq!(ffi_new, 1);
                    }
                    LockDecision::Reentrant => {
                        assert_eq!(ffi_ret, OK);
                        #[allow(clippy::arithmetic_side_effects)]
                        let expected = lock_count + 1;
                        assert_eq!(ffi_new, expected);
                    }
                    LockDecision::Overflow => {
                        assert_eq!(ffi_ret, EINVAL);
                    }
                    LockDecision::Busy | LockDecision::Pend => {
                        assert_eq!(ffi_ret, EBUSY);
                    }
                }
            }
        }
    }
}

// =====================================================================
// Differential tests: mutex unlock_decide
// =====================================================================

#[test]
fn mutex_unlock_decide_ffi_matches_model_exhaustive() {
    let lock_counts: &[u32] = &[0, 1, 2, 3, 5];
    for &lock_count in lock_counts {
        for owner_is_null in [false, true] {
            for owner_is_current in [false, true] {
                if owner_is_null && owner_is_current {
                    continue;
                }

                let ffi_d =
                    ffi_mutex_unlock_decide(lock_count, owner_is_null, owner_is_current);
                let model_d =
                    mutex::unlock_decide(lock_count, owner_is_null, owner_is_current);

                match model_d {
                    UnlockDecisionKind::NotLocked => {
                        assert_eq!(ffi_d.ret, EINVAL, "NotLocked: ret");
                        assert_eq!(ffi_d.action, FFI_MUTEX_UNLOCK_ERROR, "NotLocked: action");
                    }
                    UnlockDecisionKind::NotOwner => {
                        assert_eq!(ffi_d.ret, EPERM, "NotOwner: ret");
                        assert_eq!(ffi_d.action, FFI_MUTEX_UNLOCK_ERROR, "NotOwner: action");
                        assert_eq!(
                            ffi_d.new_lock_count, lock_count,
                            "NotOwner: lock_count unchanged"
                        );
                    }
                    UnlockDecisionKind::Released => {
                        assert_eq!(ffi_d.ret, OK, "Released: ret");
                        assert_eq!(ffi_d.action, FFI_MUTEX_UNLOCK_RELEASED, "Released: action");
                        #[allow(clippy::arithmetic_side_effects)]
                        let expected = lock_count - 1;
                        assert_eq!(ffi_d.new_lock_count, expected, "Released: new_lock_count");
                    }
                    UnlockDecisionKind::FullyUnlocked => {
                        assert_eq!(ffi_d.ret, OK, "FullyUnlocked: ret");
                        assert_eq!(
                            ffi_d.action, FFI_MUTEX_UNLOCK_UNLOCKED,
                            "FullyUnlocked: action"
                        );
                        assert_eq!(ffi_d.new_lock_count, 0, "FullyUnlocked: new_lock_count");
                    }
                }
            }
        }
    }
}

// =====================================================================
// Differential tests: mutex unlock_validate (legacy API)
// =====================================================================

#[test]
fn mutex_unlock_validate_ffi_matches_model_exhaustive() {
    let lock_counts: &[u32] = &[0, 1, 2, 3];
    for &lock_count in lock_counts {
        for owner_is_null in [false, true] {
            for owner_is_current in [false, true] {
                if owner_is_null && owner_is_current {
                    continue;
                }

                let (ffi_ret, ffi_new) =
                    ffi_mutex_unlock_validate(lock_count, owner_is_null, owner_is_current);
                let model_d =
                    mutex::unlock_decide(lock_count, owner_is_null, owner_is_current);

                match model_d {
                    UnlockDecisionKind::NotLocked => {
                        assert_eq!(ffi_ret, EINVAL);
                    }
                    UnlockDecisionKind::NotOwner => {
                        assert_eq!(ffi_ret, EPERM);
                    }
                    UnlockDecisionKind::Released => {
                        assert_eq!(ffi_ret, 1); // GALE_MUTEX_RELEASED
                        #[allow(clippy::arithmetic_side_effects)]
                        let expected = lock_count - 1;
                        assert_eq!(ffi_new, expected);
                    }
                    UnlockDecisionKind::FullyUnlocked => {
                        assert_eq!(ffi_ret, 0); // GALE_MUTEX_UNLOCKED
                        assert_eq!(ffi_new, 0);
                    }
                }
            }
        }
    }
}

// =====================================================================
// Property: M1 — owner iff lock_count > 0
// =====================================================================

#[test]
fn mutex_lock_then_unlock_roundtrip() {
    // Acquire (unlocked) then unlock
    let lock_d = ffi_mutex_lock_decide(0, true, false, false);
    assert_eq!(lock_d.action, FFI_MUTEX_ACTION_ACQUIRED);
    assert_eq!(lock_d.new_lock_count, 1);

    // Unlock: owner_is_null=false (we hold it), owner_is_current=true
    let unlock_d = ffi_mutex_unlock_decide(lock_d.new_lock_count, false, true);
    assert_eq!(unlock_d.action, FFI_MUTEX_UNLOCK_UNLOCKED);
    assert_eq!(unlock_d.new_lock_count, 0);
}

// =====================================================================
// Property: M4 — reentrancy: N locks -> lock_count == N
// =====================================================================

#[test]
fn mutex_reentrant_lock_count_tracks_depth() {
    let mut lock_count = 0u32;
    // First lock (acquire)
    let d = ffi_mutex_lock_decide(lock_count, true, false, false);
    assert_eq!(d.new_lock_count, 1);
    lock_count = d.new_lock_count;

    // Reentrant locks 2..=5
    for expected in 2u32..=5 {
        let d = ffi_mutex_lock_decide(lock_count, false, true, false);
        assert_eq!(d.action, FFI_MUTEX_ACTION_ACQUIRED);
        assert_eq!(d.new_lock_count, expected);
        lock_count = d.new_lock_count;
    }

    // Reentrant unlocks back to 0
    for expected in (0u32..=4).rev() {
        let d = ffi_mutex_unlock_decide(lock_count, false, true);
        if expected == 0 {
            assert_eq!(d.action, FFI_MUTEX_UNLOCK_UNLOCKED);
            assert_eq!(d.new_lock_count, 0);
        } else {
            assert_eq!(d.action, FFI_MUTEX_UNLOCK_RELEASED);
            assert_eq!(d.new_lock_count, expected);
        }
        lock_count = d.new_lock_count;
    }

    assert_eq!(lock_count, 0);
}

// =====================================================================
// Property: M10 — no overflow at u32::MAX
// =====================================================================

#[test]
fn mutex_lock_overflow_protection() {
    let d = ffi_mutex_lock_decide(u32::MAX, false, true, false);
    let model_d = mutex::lock_decide(u32::MAX, false, true, false);
    assert_eq!(model_d, LockDecision::Overflow);
    assert_eq!(d.ret, EINVAL, "overflow must return EINVAL");
}
