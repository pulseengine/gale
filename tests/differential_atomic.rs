//! Differential equivalence tests — Atomic (Model self-consistency).
//!
//! Since there are no separate FFI functions for atomics (the model itself
//! is the canonical implementation), these tests verify the model's
//! internal consistency: that each operation satisfies its contract and
//! that the wrapping arithmetic helpers match Rust's wrapping_add/sub.

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

use gale::atomic::{AtomicVal, add_u32_wrapping, sub_u32_wrapping};

// =====================================================================
// FFI replicas — standalone implementations of the wrapping operations
// that the model wraps
// =====================================================================

/// Replica of atomic_add: returns old value, new = old.wrapping_add(val).
fn ffi_atomic_add(current: u32, value: u32) -> (u32, u32) {
    let old = current;
    let new = current.wrapping_add(value);
    (old, new)
}

/// Replica of atomic_sub: returns old value, new = old.wrapping_sub(val).
fn ffi_atomic_sub(current: u32, value: u32) -> (u32, u32) {
    let old = current;
    let new = current.wrapping_sub(value);
    (old, new)
}

/// Replica of atomic_or: returns old value, new = old | val.
fn ffi_atomic_or(current: u32, value: u32) -> (u32, u32) {
    (current, current | value)
}

/// Replica of atomic_and: returns old value, new = old & val.
fn ffi_atomic_and(current: u32, value: u32) -> (u32, u32) {
    (current, current & value)
}

/// Replica of atomic_xor: returns old value, new = old ^ val.
fn ffi_atomic_xor(current: u32, value: u32) -> (u32, u32) {
    (current, current ^ value)
}

/// Replica of atomic_nand: returns old value, new = !(old & val).
fn ffi_atomic_nand(current: u32, value: u32) -> (u32, u32) {
    (current, !(current & value))
}

/// Replica of atomic_cas: if current == expected, swap to new; return success.
fn ffi_atomic_cas(current: u32, expected: u32, new_value: u32) -> (bool, u32) {
    if current == expected {
        (true, new_value)
    } else {
        (false, current)
    }
}

/// Replica of atomic_set: returns old, sets new value.
fn ffi_atomic_set(current: u32, value: u32) -> (u32, u32) {
    (current, value)
}

/// Replica of atomic_get: returns current value without mutation.
fn ffi_atomic_get(current: u32) -> u32 {
    current
}

/// Replica of atomic_test_and_set: returns old, sets to 1.
fn ffi_atomic_test_and_set(current: u32) -> (u32, u32) {
    (current, 1)
}

/// Replica of atomic_clear: sets to 0.
fn ffi_atomic_clear(_current: u32) -> u32 {
    0
}

// =====================================================================
// Differential tests: wrapping arithmetic helpers
// =====================================================================

#[test]
fn atomic_wrapping_add_matches_rust_wrapping_add_exhaustive() {
    // Test a set of boundary and non-boundary values
    let vals: &[u32] = &[0, 1, 127, 128, 255, 256, 0x7FFF_FFFF, 0x8000_0000, u32::MAX - 1, u32::MAX];
    for &a in vals {
        for &b in vals {
            let model_result = add_u32_wrapping(a, b);
            let rust_result = a.wrapping_add(b);
            assert_eq!(model_result, rust_result,
                "add_u32_wrapping mismatch: {a} + {b}");
        }
    }
}

#[test]
fn atomic_wrapping_sub_matches_rust_wrapping_sub_exhaustive() {
    let vals: &[u32] = &[0, 1, 127, 128, 255, 256, 0x7FFF_FFFF, 0x8000_0000, u32::MAX - 1, u32::MAX];
    for &a in vals {
        for &b in vals {
            let model_result = sub_u32_wrapping(a, b);
            let rust_result = a.wrapping_sub(b);
            assert_eq!(model_result, rust_result,
                "sub_u32_wrapping mismatch: {a} - {b}");
        }
    }
}

// =====================================================================
// Differential tests: add — replica vs model
// =====================================================================

#[test]
fn atomic_add_ffi_matches_model_exhaustive() {
    let vals: &[u32] = &[0, 1, 10, 100, u32::MAX - 1, u32::MAX];
    for &initial in vals {
        for &delta in vals {
            let (ffi_old, ffi_new) = ffi_atomic_add(initial, delta);

            let mut atom = AtomicVal::new(initial);
            let model_old = atom.add(delta);
            let model_new = atom.val;

            assert_eq!(ffi_old, model_old,
                "add old mismatch: initial={initial}, delta={delta}");
            assert_eq!(ffi_new, model_new,
                "add new mismatch: initial={initial}, delta={delta}");
        }
    }
}

// =====================================================================
// Differential tests: sub — replica vs model
// =====================================================================

#[test]
fn atomic_sub_ffi_matches_model_exhaustive() {
    let vals: &[u32] = &[0, 1, 10, 100, u32::MAX - 1, u32::MAX];
    for &initial in vals {
        for &delta in vals {
            let (ffi_old, ffi_new) = ffi_atomic_sub(initial, delta);

            let mut atom = AtomicVal::new(initial);
            let model_old = atom.sub(delta);
            let model_new = atom.val;

            assert_eq!(ffi_old, model_old,
                "sub old mismatch: initial={initial}, delta={delta}");
            assert_eq!(ffi_new, model_new,
                "sub new mismatch: initial={initial}, delta={delta}");
        }
    }
}

// =====================================================================
// Differential tests: or / and / xor / nand — replica vs model
// =====================================================================

#[test]
fn atomic_or_ffi_matches_model_exhaustive() {
    let vals: &[u32] = &[0, 1, 0xFF, 0xF0F0_F0F0, 0xFFFF_FFFF];
    for &initial in vals {
        for &v in vals {
            let (ffi_old, ffi_new) = ffi_atomic_or(initial, v);
            let mut atom = AtomicVal::new(initial);
            let model_old = atom.or(v);
            assert_eq!(ffi_old, model_old, "or old: {initial} | {v}");
            assert_eq!(ffi_new, atom.val, "or new: {initial} | {v}");
        }
    }
}

#[test]
fn atomic_and_ffi_matches_model_exhaustive() {
    let vals: &[u32] = &[0, 1, 0xFF, 0xF0F0_F0F0, 0xFFFF_FFFF];
    for &initial in vals {
        for &v in vals {
            let (ffi_old, ffi_new) = ffi_atomic_and(initial, v);
            let mut atom = AtomicVal::new(initial);
            let model_old = atom.and(v);
            assert_eq!(ffi_old, model_old, "and old: {initial} & {v}");
            assert_eq!(ffi_new, atom.val, "and new: {initial} & {v}");
        }
    }
}

#[test]
fn atomic_xor_ffi_matches_model_exhaustive() {
    let vals: &[u32] = &[0, 1, 0xFF, 0xF0F0_F0F0, 0xFFFF_FFFF];
    for &initial in vals {
        for &v in vals {
            let (ffi_old, ffi_new) = ffi_atomic_xor(initial, v);
            let mut atom = AtomicVal::new(initial);
            let model_old = atom.xor(v);
            assert_eq!(ffi_old, model_old, "xor old: {initial} ^ {v}");
            assert_eq!(ffi_new, atom.val, "xor new: {initial} ^ {v}");
        }
    }
}

#[test]
fn atomic_nand_ffi_matches_model_exhaustive() {
    let vals: &[u32] = &[0, 1, 0xFF, 0xF0F0_F0F0, 0xFFFF_FFFF];
    for &initial in vals {
        for &v in vals {
            let (ffi_old, ffi_new) = ffi_atomic_nand(initial, v);
            let mut atom = AtomicVal::new(initial);
            let model_old = atom.nand(v);
            assert_eq!(ffi_old, model_old, "nand old: !({initial} & {v})");
            assert_eq!(ffi_new, atom.val, "nand new: !({initial} & {v})");
        }
    }
}

// =====================================================================
// Differential tests: cas — replica vs model
// =====================================================================

#[test]
fn atomic_cas_ffi_matches_model_exhaustive() {
    let vals: &[u32] = &[0, 1, 42, 100, u32::MAX];
    for &initial in vals {
        for &expected in vals {
            for &new_val in vals {
                let (ffi_success, ffi_new_state) = ffi_atomic_cas(initial, expected, new_val);

                let mut atom = AtomicVal::new(initial);
                let model_success = atom.cas(expected, new_val);
                let model_new_state = atom.val;

                assert_eq!(ffi_success, model_success,
                    "cas success mismatch: initial={initial}, exp={expected}, new={new_val}");
                assert_eq!(ffi_new_state, model_new_state,
                    "cas state mismatch: initial={initial}, exp={expected}, new={new_val}");
            }
        }
    }
}

// =====================================================================
// Differential tests: set / get — replica vs model
// =====================================================================

#[test]
fn atomic_set_ffi_matches_model_exhaustive() {
    let vals: &[u32] = &[0, 1, 42, u32::MAX];
    for &initial in vals {
        for &new_val in vals {
            let (ffi_old, ffi_new) = ffi_atomic_set(initial, new_val);

            let mut atom = AtomicVal::new(initial);
            let model_old = atom.set(new_val);

            assert_eq!(ffi_old, model_old,
                "set old mismatch: initial={initial}, new={new_val}");
            assert_eq!(ffi_new, atom.val,
                "set new mismatch: initial={initial}, new={new_val}");
        }
    }
}

#[test]
fn atomic_get_ffi_matches_model_exhaustive() {
    let vals: &[u32] = &[0, 1, 42, u32::MAX];
    for &initial in vals {
        let ffi_val = ffi_atomic_get(initial);
        let atom = AtomicVal::new(initial);
        let model_val = atom.get();
        assert_eq!(ffi_val, model_val,
            "get mismatch: initial={initial}");
    }
}

// =====================================================================
// Differential tests: test_and_set / clear — replica vs model
// =====================================================================

#[test]
fn atomic_test_and_set_ffi_matches_model_exhaustive() {
    let vals: &[u32] = &[0, 1, 42, u32::MAX];
    for &initial in vals {
        let (ffi_old, ffi_new) = ffi_atomic_test_and_set(initial);

        let mut atom = AtomicVal::new(initial);
        let model_old = atom.test_and_set();

        assert_eq!(ffi_old, model_old,
            "test_and_set old mismatch: initial={initial}");
        assert_eq!(ffi_new, atom.val,
            "test_and_set new mismatch: initial={initial}");
        assert_eq!(atom.val, 1u32,
            "AT5: test_and_set must store 1: initial={initial}");
    }
}

#[test]
fn atomic_clear_ffi_matches_model_exhaustive() {
    let vals: &[u32] = &[0, 1, 42, u32::MAX];
    for &initial in vals {
        let ffi_new = ffi_atomic_clear(initial);

        let mut atom = AtomicVal::new(initial);
        atom.clear();

        assert_eq!(ffi_new, atom.val,
            "clear mismatch: initial={initial}");
        assert_eq!(atom.val, 0u32,
            "clear must store 0: initial={initial}");
    }
}

// =====================================================================
// Property: AT1 — add returns old value
// =====================================================================

#[test]
fn atomic_add_returns_old_value() {
    for initial in [0u32, 1, u32::MAX - 1, u32::MAX] {
        for delta in [0u32, 1, u32::MAX] {
            let mut atom = AtomicVal::new(initial);
            let old = atom.add(delta);
            assert_eq!(old, initial,
                "AT1: add must return old value: initial={initial}");
        }
    }
}

// =====================================================================
// Property: AT2 — sub returns old value
// =====================================================================

#[test]
fn atomic_sub_returns_old_value() {
    for initial in [0u32, 1, u32::MAX] {
        for delta in [0u32, 1, u32::MAX] {
            let mut atom = AtomicVal::new(initial);
            let old = atom.sub(delta);
            assert_eq!(old, initial,
                "AT2: sub must return old value: initial={initial}");
        }
    }
}

// =====================================================================
// Property: AT3 — cas succeeds only when current == expected
// =====================================================================

#[test]
fn atomic_cas_succeeds_iff_values_match() {
    for initial in [0u32, 42, u32::MAX] {
        for expected in [0u32, 42, u32::MAX] {
            let mut atom = AtomicVal::new(initial);
            let success = atom.cas(expected, 999);
            assert_eq!(success, initial == expected,
                "AT3: cas success iff current==expected: initial={initial}, exp={expected}");
        }
    }
}

// =====================================================================
// Property: AT4 — cas failure leaves value unchanged
// =====================================================================

#[test]
fn atomic_cas_failure_leaves_value_unchanged() {
    let initial = 42u32;
    let mut atom = AtomicVal::new(initial);
    let success = atom.cas(99, 0);
    assert!(!success, "AT4: cas should fail when values differ");
    assert_eq!(atom.val, initial, "AT4: cas failure must not change value");
}

// =====================================================================
// Property: AT5 — test_and_set always sets to 1
// =====================================================================

#[test]
fn atomic_test_and_set_always_stores_one() {
    for initial in [0u32, 1, 42, u32::MAX] {
        let mut atom = AtomicVal::new(initial);
        atom.test_and_set();
        assert_eq!(atom.val, 1u32,
            "AT5: test_and_set must store 1 for initial={initial}");
    }
}

// =====================================================================
// Property: AT6 — add/sub wrapping roundtrip
// =====================================================================

#[test]
fn atomic_add_sub_wrapping_roundtrip() {
    let pairs: &[(u32, u32)] = &[
        (0, 0),
        (0, 1),
        (1, u32::MAX),
        (u32::MAX, 1),
        (u32::MAX, u32::MAX),
        (100, 50),
    ];
    for &(initial, delta) in pairs {
        let mut atom = AtomicVal::new(initial);
        atom.add(delta);
        atom.sub(delta);
        assert_eq!(atom.val, initial,
            "AT6: add then sub roundtrip: initial={initial}, delta={delta}");
    }
}

// =====================================================================
// Property: XOR self-inverse
// =====================================================================

#[test]
fn atomic_xor_self_inverse() {
    let vals: &[u32] = &[0, 1, 0xFF, 0xF0F0_F0F0, u32::MAX];
    for &initial in vals {
        for &v in vals {
            let mut atom = AtomicVal::new(initial);
            atom.xor(v);
            atom.xor(v);
            assert_eq!(atom.val, initial,
                "xor self-inverse: initial={initial}, v={v}");
        }
    }
}

// =====================================================================
// Property: inc/dec are add/sub 1 aliases
// =====================================================================

#[test]
fn atomic_inc_dec_match_add_sub_one() {
    let vals: &[u32] = &[0, 1, 42, u32::MAX - 1, u32::MAX];
    for &initial in vals {
        // inc vs add(1)
        let mut a1 = AtomicVal::new(initial);
        let mut a2 = AtomicVal::new(initial);
        a1.inc();
        a2.add(1);
        assert_eq!(a1.val, a2.val, "inc must equal add(1): initial={initial}");

        // dec vs sub(1)
        let mut b1 = AtomicVal::new(initial);
        let mut b2 = AtomicVal::new(initial);
        b1.dec();
        b2.sub(1);
        assert_eq!(b1.val, b2.val, "dec must equal sub(1): initial={initial}");
    }
}
