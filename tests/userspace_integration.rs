//! Integration tests for the userspace syscall validation model.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::error::*;
use gale::userspace::{InitCheck, KernelObject, MAX_THREADS, ObjType};

/// Helper: check all permissions are false.
fn all_perms_clear(ko: &KernelObject) -> bool {
    ko.thread_perms.iter().all(|&p| !p)
}

/// Helper: check all permissions are true.
fn all_perms_set(ko: &KernelObject) -> bool {
    ko.thread_perms.iter().all(|&p| p)
}

// ==================================================================
// Constructor tests (US6)
// ==================================================================

#[test]
fn new_creates_uninitialized_no_perms() {
    let ko = KernelObject::new(ObjType::Sem);
    assert_eq!(ko.obj_type, ObjType::Sem);
    assert!(!ko.flag_initialized);
    assert!(!ko.flag_public);
    assert!(!ko.flag_alloc);
    assert!(!ko.flag_driver);
    assert!(all_perms_clear(&ko));
    assert!(!ko.is_initialized());
    assert!(!ko.is_public());
}

#[test]
fn new_each_type() {
    let types = [
        ObjType::Any,
        ObjType::Thread,
        ObjType::Sem,
        ObjType::Mutex,
        ObjType::CondVar,
        ObjType::MsgQ,
        ObjType::Stack,
        ObjType::Pipe,
        ObjType::Timer,
        ObjType::Event,
        ObjType::MemSlab,
        ObjType::Fifo,
        ObjType::Lifo,
        ObjType::SysMutex,
        ObjType::Futex,
        ObjType::Mbox,
    ];
    for t in types {
        let ko = KernelObject::new(t);
        assert_eq!(ko.obj_type_get(), t);
        assert!(all_perms_clear(&ko));
    }
}

// ==================================================================
// Grant access tests (US2)
// ==================================================================

#[test]
fn us2_grant_sets_permission_bit() {
    let mut ko = KernelObject::new(ObjType::Sem);
    assert_eq!(ko.grant_access(0), OK);
    assert!(ko.has_perm(0));
    assert!(ko.thread_perms[0]);
}

#[test]
fn us2_grant_multiple_threads() {
    let mut ko = KernelObject::new(ObjType::Mutex);
    ko.grant_access(0);
    ko.grant_access(5);
    ko.grant_access(63);
    assert!(ko.has_perm(0));
    assert!(ko.has_perm(5));
    assert!(ko.has_perm(63));
    assert!(!ko.has_perm(1));
    assert!(!ko.has_perm(62));
}

#[test]
fn us2_grant_idempotent() {
    let mut ko = KernelObject::new(ObjType::Sem);
    ko.grant_access(10);
    let perms_after_first = ko.thread_perms;
    ko.grant_access(10);
    assert_eq!(ko.thread_perms, perms_after_first);
}

#[test]
fn us8_grant_invalid_tid_returns_einval() {
    let mut ko = KernelObject::new(ObjType::Sem);
    assert_eq!(ko.grant_access(MAX_THREADS), EINVAL);
    assert_eq!(ko.grant_access(MAX_THREADS + 100), EINVAL);
    assert!(all_perms_clear(&ko));
}

// ==================================================================
// Revoke access tests (US3)
// ==================================================================

#[test]
fn us3_revoke_clears_permission_bit() {
    let mut ko = KernelObject::new(ObjType::Sem);
    ko.grant_access(5);
    assert!(ko.has_perm(5));
    assert_eq!(ko.revoke_access(5), OK);
    assert!(!ko.has_perm(5));
}

#[test]
fn us3_revoke_preserves_other_bits() {
    let mut ko = KernelObject::new(ObjType::Sem);
    ko.grant_access(0);
    ko.grant_access(10);
    ko.grant_access(63);
    ko.revoke_access(10);
    assert!(ko.has_perm(0));
    assert!(!ko.has_perm(10));
    assert!(ko.has_perm(63));
}

#[test]
fn us3_revoke_idempotent() {
    let mut ko = KernelObject::new(ObjType::Sem);
    ko.grant_access(5);
    ko.revoke_access(5);
    let perms_after_first = ko.thread_perms;
    ko.revoke_access(5);
    assert_eq!(ko.thread_perms, perms_after_first);
}

#[test]
fn us8_revoke_invalid_tid_returns_einval() {
    let mut ko = KernelObject::new(ObjType::Sem);
    ko.grant_access(0);
    assert_eq!(ko.revoke_access(MAX_THREADS), EINVAL);
    assert!(ko.has_perm(0)); // unchanged
}

// ==================================================================
// Grant + revoke roundtrip (US2 + US3)
// ==================================================================

#[test]
fn grant_revoke_roundtrip() {
    let mut ko = KernelObject::new(ObjType::Sem);
    let original_perms = ko.thread_perms;
    ko.grant_access(7);
    ko.revoke_access(7);
    assert_eq!(ko.thread_perms, original_perms);
}

// ==================================================================
// Clear all perms
// ==================================================================

#[test]
fn clear_all_perms_resets_bitmask() {
    let mut ko = KernelObject::new(ObjType::Sem);
    ko.grant_access(0);
    ko.grant_access(31);
    ko.grant_access(63);
    ko.clear_all_perms();
    assert!(all_perms_clear(&ko));
}

#[test]
fn clear_thread_perm_single() {
    let mut ko = KernelObject::new(ObjType::Sem);
    ko.grant_access(5);
    ko.grant_access(10);
    ko.clear_thread_perm(5);
    assert!(!ko.has_perm(5));
    assert!(ko.has_perm(10));
}

// ==================================================================
// Access check tests (US1, US5)
// ==================================================================

#[test]
fn us1_access_denied_without_permission() {
    let ko = KernelObject::new(ObjType::Sem);
    assert!(!ko.check_access(0, false));
    assert!(!ko.check_access(63, false));
}

#[test]
fn us1_access_granted_with_permission() {
    let mut ko = KernelObject::new(ObjType::Sem);
    ko.grant_access(5);
    assert!(ko.check_access(5, false));
    assert!(!ko.check_access(6, false));
}

#[test]
fn us5_supervisor_bypasses_permission() {
    let ko = KernelObject::new(ObjType::Sem);
    // No permissions set, but supervisor -> granted
    assert!(ko.check_access(0, true));
    assert!(ko.check_access(MAX_THREADS + 100, true));
}

#[test]
fn us5_public_object_grants_all() {
    let mut ko = KernelObject::new(ObjType::Sem);
    ko.make_public();
    // No per-thread perms set, but public -> granted
    assert!(ko.check_access(0, false));
    assert!(ko.check_access(63, false));
}

#[test]
fn us8_invalid_tid_access_denied() {
    let ko = KernelObject::new(ObjType::Sem);
    // Not public, not supervisor, invalid tid
    assert!(!ko.check_access(MAX_THREADS, false));
    assert!(!ko.check_access(u32::MAX, false));
}

// ==================================================================
// Object validation tests (US4, US7)
// ==================================================================

#[test]
fn us4_type_mismatch_returns_ebadf() {
    let mut ko = KernelObject::new(ObjType::Sem);
    ko.grant_access(0);
    ko.init_object();
    let result = ko.validate(ObjType::Mutex, 0, false, InitCheck::MustBeInit);
    assert_eq!(result, Err(EBADF));
}

#[test]
fn us4_any_type_matches_anything() {
    let mut ko = KernelObject::new(ObjType::Sem);
    ko.grant_access(0);
    ko.init_object();
    let result = ko.validate(ObjType::Any, 0, false, InitCheck::MustBeInit);
    assert_eq!(result, Ok(()));
}

#[test]
fn us4_exact_type_match_succeeds() {
    let mut ko = KernelObject::new(ObjType::Mutex);
    ko.grant_access(0);
    ko.init_object();
    let result = ko.validate(ObjType::Mutex, 0, false, InitCheck::MustBeInit);
    assert_eq!(result, Ok(()));
}

#[test]
fn us1_no_permission_returns_eperm() {
    let mut ko = KernelObject::new(ObjType::Sem);
    ko.init_object();
    // Thread 0 has no permission
    let result = ko.validate(ObjType::Sem, 0, false, InitCheck::MustBeInit);
    assert_eq!(result, Err(EPERM));
}

#[test]
fn us7_uninit_must_be_init_returns_einval() {
    let mut ko = KernelObject::new(ObjType::Sem);
    ko.grant_access(0);
    // Object is NOT initialized
    let result = ko.validate(ObjType::Sem, 0, false, InitCheck::MustBeInit);
    assert_eq!(result, Err(EINVAL));
}

#[test]
fn us7_init_must_not_be_init_returns_eaddrinuse() {
    let mut ko = KernelObject::new(ObjType::Sem);
    ko.grant_access(0);
    ko.init_object();
    let result = ko.validate(ObjType::Sem, 0, false, InitCheck::MustNotBeInit);
    assert_eq!(result, Err(EADDRINUSE));
}

#[test]
fn us7_dont_care_ignores_init_state() {
    let mut ko = KernelObject::new(ObjType::Sem);
    ko.grant_access(0);
    // Not initialized — DontCare should pass
    let result = ko.validate(ObjType::Sem, 0, false, InitCheck::DontCare);
    assert_eq!(result, Ok(()));

    // Initialize and try again
    ko.init_object();
    let result2 = ko.validate(ObjType::Sem, 0, false, InitCheck::DontCare);
    assert_eq!(result2, Ok(()));
}

#[test]
fn validate_supervisor_bypasses_perms_but_not_type() {
    let mut ko = KernelObject::new(ObjType::Sem);
    ko.init_object();
    // Supervisor mode, type mismatch -> still EBADF
    let result = ko.validate(ObjType::Mutex, 0, true, InitCheck::MustBeInit);
    assert_eq!(result, Err(EBADF));
}

#[test]
fn validate_supervisor_bypasses_perms_and_init() {
    let ko = KernelObject::new(ObjType::Sem);
    // Not initialized, no perms, but supervisor + DontCare -> Ok
    let result = ko.validate(ObjType::Sem, 0, true, InitCheck::DontCare);
    assert_eq!(result, Ok(()));
}

#[test]
fn validate_supervisor_still_checks_init() {
    let ko = KernelObject::new(ObjType::Sem);
    // Supervisor mode bypasses perms, but MustBeInit still fails
    let result = ko.validate(ObjType::Sem, 0, true, InitCheck::MustBeInit);
    assert_eq!(result, Err(EINVAL));
}

// ==================================================================
// Initialization tests
// ==================================================================

#[test]
fn init_sets_flag() {
    let mut ko = KernelObject::new(ObjType::Sem);
    assert!(!ko.is_initialized());
    ko.init_object();
    assert!(ko.is_initialized());
}

#[test]
fn uninit_clears_flag() {
    let mut ko = KernelObject::new(ObjType::Sem);
    ko.init_object();
    assert!(ko.is_initialized());
    ko.uninit_object();
    assert!(!ko.is_initialized());
}

#[test]
fn init_preserves_perms() {
    let mut ko = KernelObject::new(ObjType::Sem);
    ko.grant_access(5);
    let perms_before = ko.thread_perms;
    ko.init_object();
    assert_eq!(ko.thread_perms, perms_before);
}

#[test]
fn uninit_preserves_perms() {
    let mut ko = KernelObject::new(ObjType::Sem);
    ko.grant_access(5);
    ko.init_object();
    let perms_before = ko.thread_perms;
    ko.uninit_object();
    assert_eq!(ko.thread_perms, perms_before);
}

// ==================================================================
// Recycle tests
// ==================================================================

#[test]
fn recycle_clears_perms_grants_current_inits() {
    let mut ko = KernelObject::new(ObjType::Sem);
    ko.grant_access(0);
    ko.grant_access(5);
    ko.grant_access(63);

    assert_eq!(ko.recycle(10), OK);
    // Only thread 10 should have access
    assert!(!ko.has_perm(0));
    assert!(!ko.has_perm(5));
    assert!(ko.has_perm(10));
    assert!(!ko.has_perm(63));
    // Should be initialized
    assert!(ko.is_initialized());
    // Type preserved
    assert_eq!(ko.obj_type, ObjType::Sem);
}

#[test]
fn recycle_works_on_uninitialized() {
    let mut ko = KernelObject::new(ObjType::Mutex);
    assert!(!ko.is_initialized());
    ko.recycle(0);
    assert!(ko.is_initialized());
    assert!(ko.has_perm(0));
}

// ==================================================================
// Make public tests
// ==================================================================

#[test]
fn make_public_sets_flag() {
    let mut ko = KernelObject::new(ObjType::Sem);
    assert!(!ko.is_public());
    ko.make_public();
    assert!(ko.is_public());
}

#[test]
fn public_preserves_perms() {
    let mut ko = KernelObject::new(ObjType::Sem);
    ko.grant_access(5);
    let perms_before = ko.thread_perms;
    ko.make_public();
    assert_eq!(ko.thread_perms, perms_before);
}

// ==================================================================
// Edge cases
// ==================================================================

#[test]
fn all_threads_granted() {
    let mut ko = KernelObject::new(ObjType::Sem);
    for i in 0..MAX_THREADS {
        ko.grant_access(i);
    }
    assert!(all_perms_set(&ko));
    for i in 0..MAX_THREADS {
        assert!(ko.has_perm(i));
    }
}

#[test]
fn validate_full_happy_path() {
    let mut ko = KernelObject::new(ObjType::Pipe);
    ko.grant_access(3);
    ko.init_object();
    let result = ko.validate(ObjType::Pipe, 3, false, InitCheck::MustBeInit);
    assert_eq!(result, Ok(()));
}

#[test]
fn validate_priority_type_before_perms() {
    // Type check happens before permission check
    let ko = KernelObject::new(ObjType::Sem);
    // No perms at all, but type mismatch -> EBADF (not EPERM)
    let result = ko.validate(ObjType::Mutex, 0, false, InitCheck::DontCare);
    assert_eq!(result, Err(EBADF));
}

#[test]
fn validate_priority_perms_before_init() {
    // Permission check happens before init check
    let ko = KernelObject::new(ObjType::Sem);
    // Type matches, no perms, not initialized
    let result = ko.validate(ObjType::Sem, 0, false, InitCheck::MustBeInit);
    // Should get EPERM (no permission), not EINVAL (not initialized)
    assert_eq!(result, Err(EPERM));
}

#[test]
fn thread_zero_permission() {
    let mut ko = KernelObject::new(ObjType::Sem);
    ko.grant_access(0);
    assert!(ko.has_perm(0));
    assert!(ko.thread_perms[0]);
}

#[test]
fn thread_63_permission() {
    let mut ko = KernelObject::new(ObjType::Sem);
    ko.grant_access(63);
    assert!(ko.has_perm(63));
    assert!(ko.thread_perms[63]);
}
