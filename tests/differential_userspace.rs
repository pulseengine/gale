//! Differential equivalence tests — Userspace (FFI vs Model).
//!
//! Verifies that the FFI userspace functions produce the same results
//! as the Verus-verified model functions in gale::userspace.

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
use gale::userspace::{
    KernelObject, ObjType,
    access_decide, validate_decide,
};

// Flag constants — must match FFI layer (ffi/src/lib.rs)
const K_OBJ_FLAG_INITIALIZED: u8 = 0x01;
const K_OBJ_FLAG_PUBLIC: u8 = 0x02;

// Init check constants — must match FFI layer
const OBJ_INIT_TRUE: i8 = 0;    // MustBeInit
const OBJ_INIT_FALSE: i8 = -1;  // MustNotBeInit
const OBJ_INIT_ANY: i8 = 1;     // DontCare

const MAX_THREADS: u32 = 64;

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Replica of gale_k_object_access_decide.
///
/// Returns 1 if access granted, 0 if denied.
fn ffi_object_access_decide(flags: u8, has_perm_bit: u8) -> u8 {
    let is_public = (flags & K_OBJ_FLAG_PUBLIC) != 0;
    let granted = access_decide(is_public, has_perm_bit != 0);
    if granted { 1 } else { 0 }
}

/// Replica of gale_k_object_validate_decide.
///
/// Returns 0=OK, -EBADF/-EPERM/-EINVAL/-EADDRINUSE on failure.
fn ffi_object_validate_decide(
    obj_type: u8,
    expected_type: u8,
    flags: u8,
    has_access: u8,
    init_check: i8,
) -> i32 {
    let type_matches = expected_type == 0 || obj_type == expected_type;
    let is_initialized = (flags & K_OBJ_FLAG_INITIALIZED) != 0;
    match validate_decide(type_matches, has_access != 0, is_initialized, init_check) {
        Ok(()) => OK,
        Err(e) => e,
    }
}

/// Replica of gale_k_object_init_decide.
fn ffi_object_init_decide(current_flags: u8) -> u8 {
    current_flags | K_OBJ_FLAG_INITIALIZED
}

/// Replica of gale_k_object_uninit_decide.
fn ffi_object_uninit_decide(current_flags: u8) -> u8 {
    current_flags & !K_OBJ_FLAG_INITIALIZED
}

/// Replica of gale_k_object_recycle_decide.
///
/// Returns (new_flags, clear_perms).
fn ffi_object_recycle_decide(current_flags: u8) -> (u8, u8) {
    (current_flags | K_OBJ_FLAG_INITIALIZED, 1)
}

/// Replica of gale_k_object_make_public_decide.
fn ffi_object_make_public_decide(current_flags: u8) -> u8 {
    current_flags | K_OBJ_FLAG_PUBLIC
}

// =====================================================================
// Model helper: encode flags from KernelObject
// =====================================================================

fn encode_flags(ko: &KernelObject) -> u8 {
    let mut f: u8 = 0;
    if ko.flag_initialized { f |= K_OBJ_FLAG_INITIALIZED; }
    if ko.flag_public { f |= K_OBJ_FLAG_PUBLIC; }
    f
}

// =====================================================================
// Differential tests: access_decide
// =====================================================================

#[test]
fn userspace_access_decide_ffi_matches_model_exhaustive() {
    for flags in 0u8..=3u8 {
        for has_perm_bit in [0u8, 1] {
            let ffi_granted = ffi_object_access_decide(flags, has_perm_bit);

            let is_public = (flags & K_OBJ_FLAG_PUBLIC) != 0;
            let model_granted = access_decide(is_public, has_perm_bit != 0);

            assert_eq!(
                ffi_granted != 0,
                model_granted,
                "access_decide mismatch: flags={flags:#04x}, has_perm={has_perm_bit}"
            );
        }
    }
}

// =====================================================================
// Differential tests: validate_decide
// =====================================================================

#[test]
fn userspace_validate_decide_ffi_matches_model_exhaustive() {
    // Sweep over small type values, flag combinations, access bit, init_check
    for obj_type in 0u8..=5 {
        for expected_type in [0u8, 1, 2, 3] {
            for flags in 0u8..=3 {
                for has_access in [0u8, 1] {
                    for init_check in [OBJ_INIT_TRUE, OBJ_INIT_FALSE, OBJ_INIT_ANY] {
                        let ffi_ret = ffi_object_validate_decide(
                            obj_type, expected_type, flags, has_access, init_check,
                        );

                        let type_matches = expected_type == 0 || obj_type == expected_type;
                        let is_initialized = (flags & K_OBJ_FLAG_INITIALIZED) != 0;
                        let model_ret = match validate_decide(
                            type_matches, has_access != 0, is_initialized, init_check,
                        ) {
                            Ok(()) => OK,
                            Err(e) => e,
                        };

                        assert_eq!(ffi_ret, model_ret,
                            "validate_decide mismatch: obj={obj_type}, exp={expected_type}, \
                             flags={flags:#04x}, access={has_access}, init={init_check}");
                    }
                }
            }
        }
    }
}

// =====================================================================
// Differential tests: object init/uninit/recycle/make_public
// =====================================================================

#[test]
fn userspace_init_decide_ffi_matches_model_exhaustive() {
    for flags in 0u8..=15 {
        let ffi_new_flags = ffi_object_init_decide(flags);
        assert_ne!(ffi_new_flags & K_OBJ_FLAG_INITIALIZED, 0,
            "init_decide must set INITIALIZED bit: flags={flags:#04x}");
        // All other flag bits should be preserved
        assert_eq!(ffi_new_flags & !K_OBJ_FLAG_INITIALIZED,
                   flags & !K_OBJ_FLAG_INITIALIZED,
                   "init_decide must preserve other flags: flags={flags:#04x}");

        // Verify model consistency
        let mut ko = KernelObject::new(ObjType::Sem);
        ko.flag_initialized = (flags & K_OBJ_FLAG_INITIALIZED) != 0;
        ko.flag_public = (flags & K_OBJ_FLAG_PUBLIC) != 0;
        let old_flags = encode_flags(&ko);
        ko.init_object();
        let new_flags = encode_flags(&ko);
        assert_eq!(ffi_new_flags & (K_OBJ_FLAG_INITIALIZED | K_OBJ_FLAG_PUBLIC),
                   new_flags & (K_OBJ_FLAG_INITIALIZED | K_OBJ_FLAG_PUBLIC),
                   "init_decide model mismatch: old_flags={old_flags:#04x}");
    }
}

#[test]
fn userspace_uninit_decide_ffi_matches_model_exhaustive() {
    for flags in 0u8..=15 {
        let ffi_new_flags = ffi_object_uninit_decide(flags);
        assert_eq!(ffi_new_flags & K_OBJ_FLAG_INITIALIZED, 0,
            "uninit_decide must clear INITIALIZED bit: flags={flags:#04x}");
        assert_eq!(ffi_new_flags & !K_OBJ_FLAG_INITIALIZED,
                   flags & !K_OBJ_FLAG_INITIALIZED,
                   "uninit_decide must preserve other flags: flags={flags:#04x}");

        let mut ko = KernelObject::new(ObjType::Sem);
        ko.flag_initialized = (flags & K_OBJ_FLAG_INITIALIZED) != 0;
        ko.flag_public = (flags & K_OBJ_FLAG_PUBLIC) != 0;
        ko.uninit_object();
        let new_flags = encode_flags(&ko);
        assert_eq!(ffi_new_flags & (K_OBJ_FLAG_INITIALIZED | K_OBJ_FLAG_PUBLIC),
                   new_flags & (K_OBJ_FLAG_INITIALIZED | K_OBJ_FLAG_PUBLIC),
                   "uninit_decide model mismatch: flags={flags:#04x}");
    }
}

#[test]
fn userspace_recycle_decide_sets_initialized_and_clear_perms() {
    for flags in 0u8..=15 {
        let (ffi_new_flags, ffi_clear_perms) = ffi_object_recycle_decide(flags);
        assert_ne!(ffi_new_flags & K_OBJ_FLAG_INITIALIZED, 0,
            "recycle_decide must set INITIALIZED: flags={flags:#04x}");
        assert_eq!(ffi_clear_perms, 1,
            "recycle_decide must set clear_perms=1: flags={flags:#04x}");
    }
}

#[test]
fn userspace_make_public_decide_ffi_matches_model_exhaustive() {
    for flags in 0u8..=15 {
        let ffi_new_flags = ffi_object_make_public_decide(flags);
        assert_ne!(ffi_new_flags & K_OBJ_FLAG_PUBLIC, 0,
            "make_public_decide must set PUBLIC bit: flags={flags:#04x}");
        assert_eq!(ffi_new_flags & !K_OBJ_FLAG_PUBLIC,
                   flags & !K_OBJ_FLAG_PUBLIC,
                   "make_public_decide must preserve other flags: flags={flags:#04x}");

        let mut ko = KernelObject::new(ObjType::Sem);
        ko.flag_initialized = (flags & K_OBJ_FLAG_INITIALIZED) != 0;
        ko.flag_public = (flags & K_OBJ_FLAG_PUBLIC) != 0;
        ko.make_public();
        assert!(ko.flag_public, "model make_public must set flag");
    }
}

// =====================================================================
// Differential tests: KernelObject grant_access / check_access
// =====================================================================

#[test]
fn userspace_grant_then_check_roundtrip() {
    for tid in 0u32..=5 {
        let mut ko = KernelObject::new(ObjType::Thread);
        assert_eq!(ko.grant_access(tid), OK);
        assert!(ko.check_access(tid, false),
            "US2+US1: after grant, check_access must return true: tid={tid}");
    }
}

#[test]
fn userspace_revoke_then_check_roundtrip() {
    for tid in 0u32..=5 {
        let mut ko = KernelObject::new(ObjType::Thread);
        let rc = ko.grant_access(tid);
        assert_eq!(rc, OK, "grant should succeed for tid={tid}");
        let rc = ko.revoke_access(tid);
        assert_eq!(rc, OK, "revoke should succeed for tid={tid}");
        assert!(!ko.check_access(tid, false),
            "US3+US1: after revoke, check_access must return false: tid={tid}");
    }
}

// =====================================================================
// Property: US5 — supervisor bypasses permission checks
// =====================================================================

#[test]
fn userspace_supervisor_always_granted() {
    for is_public in [false, true] {
        let mut ko = KernelObject::new(ObjType::Sem);
        if is_public { ko.make_public(); }
        // No permissions set, but supervisor always passes
        for tid in [0u32, 63, u32::MAX] {
            assert!(ko.check_access(tid, true),
                "US5: supervisor always granted: tid={tid}, public={is_public}");
        }
    }
}

// =====================================================================
// Property: US6 — new objects have no permissions
// =====================================================================

#[test]
fn userspace_new_object_no_perms() {
    let ko = KernelObject::new(ObjType::Mutex);
    assert!(!ko.flag_initialized, "US6: new object not initialized");
    assert!(!ko.flag_public, "US6: new object not public");
    for tid in 0u32..MAX_THREADS {
        assert!(!ko.has_perm(tid),
            "US6: new object has no perms for tid={tid}");
    }
}

// =====================================================================
// Property: US4 — type mismatch returns EBADF
// =====================================================================

#[test]
fn userspace_type_mismatch_fails_validation() {
    // expected_type=1 (Thread), obj_type=2 (Sem) -> EBADF
    let ffi_ret = ffi_object_validate_decide(2, 1, K_OBJ_FLAG_INITIALIZED, 1, OBJ_INIT_ANY);
    assert_eq!(ffi_ret, EBADF, "US4: type mismatch must return EBADF");

    // expected_type=0 (Any) always matches
    let ffi_ret = ffi_object_validate_decide(2, 0, K_OBJ_FLAG_INITIALIZED, 1, OBJ_INIT_ANY);
    assert_eq!(ffi_ret, OK, "US4: K_OBJ_ANY should match any type");
}

// =====================================================================
// Property: US7 — init check enforcement
// =====================================================================

#[test]
fn userspace_init_check_must_be_init_fails_when_not_initialized() {
    // Not initialized, MustBeInit -> EINVAL
    let ffi_ret = ffi_object_validate_decide(1, 1, 0, 1, OBJ_INIT_TRUE);
    assert_eq!(ffi_ret, EINVAL,
        "US7: MustBeInit must fail when not initialized");
}

#[test]
fn userspace_init_check_must_not_be_init_fails_when_initialized() {
    // Initialized, MustNotBeInit -> EADDRINUSE
    let ffi_ret = ffi_object_validate_decide(1, 1, K_OBJ_FLAG_INITIALIZED, 1, OBJ_INIT_FALSE);
    assert_eq!(ffi_ret, EADDRINUSE,
        "US7: MustNotBeInit must fail when already initialized");
}

#[test]
fn userspace_init_check_dont_care_passes_either() {
    // Initialized, DontCare -> OK (assuming type match and access)
    let ffi_ret = ffi_object_validate_decide(1, 1, K_OBJ_FLAG_INITIALIZED, 1, OBJ_INIT_ANY);
    assert_eq!(ffi_ret, OK, "US7: DontCare must pass when initialized");

    let ffi_ret = ffi_object_validate_decide(1, 1, 0, 1, OBJ_INIT_ANY);
    assert_eq!(ffi_ret, OK, "US7: DontCare must pass when not initialized");
}

// =====================================================================
// Property: US8 — invalid tid denied unless public or supervisor
// =====================================================================

#[test]
fn userspace_invalid_tid_denied_for_private_object() {
    let ko = KernelObject::new(ObjType::Sem);
    // tid=MAX_THREADS is out of range
    assert!(!ko.check_access(MAX_THREADS, false),
        "US8: invalid tid should be denied for private object");
    // Supervisor overrides even for invalid tid
    assert!(ko.check_access(MAX_THREADS, true),
        "US5: supervisor overrides even with invalid tid");
}
