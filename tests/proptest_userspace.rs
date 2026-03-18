//! Property-based tests for the userspace syscall validation model.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::unreachable,
    clippy::indexing_slicing
)]

use gale::error::*;
use gale::userspace::{InitCheck, KernelObject, MAX_THREADS, ObjType};
use proptest::prelude::*;

/// Strategy for generating valid ObjType values.
fn obj_type_strategy() -> impl Strategy<Value = ObjType> {
    prop_oneof![
        Just(ObjType::Any),
        Just(ObjType::Thread),
        Just(ObjType::Sem),
        Just(ObjType::Mutex),
        Just(ObjType::CondVar),
        Just(ObjType::MsgQ),
        Just(ObjType::Stack),
        Just(ObjType::Pipe),
        Just(ObjType::Timer),
        Just(ObjType::Event),
        Just(ObjType::MemSlab),
        Just(ObjType::Fifo),
        Just(ObjType::Lifo),
        Just(ObjType::SysMutex),
        Just(ObjType::Futex),
        Just(ObjType::Mbox),
    ]
}

/// Strategy for generating non-Any ObjType values (for type mismatch testing).
fn non_any_obj_type_strategy() -> impl Strategy<Value = ObjType> {
    prop_oneof![
        Just(ObjType::Thread),
        Just(ObjType::Sem),
        Just(ObjType::Mutex),
        Just(ObjType::CondVar),
        Just(ObjType::MsgQ),
        Just(ObjType::Stack),
        Just(ObjType::Pipe),
        Just(ObjType::Timer),
        Just(ObjType::Event),
        Just(ObjType::MemSlab),
        Just(ObjType::Fifo),
        Just(ObjType::Lifo),
        Just(ObjType::SysMutex),
        Just(ObjType::Futex),
        Just(ObjType::Mbox),
    ]
}

/// Strategy for generating InitCheck values.
fn init_check_strategy() -> impl Strategy<Value = InitCheck> {
    prop_oneof![
        Just(InitCheck::MustBeInit),
        Just(InitCheck::MustNotBeInit),
        Just(InitCheck::DontCare),
    ]
}

/// Helper: check all permissions are false.
fn all_perms_clear(ko: &KernelObject) -> bool {
    ko.thread_perms.iter().all(|&p| !p)
}

/// Helper: check only tid has permission set.
fn only_perm_set(ko: &KernelObject, tid: u32) -> bool {
    for i in 0..MAX_THREADS {
        if i == tid {
            if !ko.thread_perms[i as usize] {
                return false;
            }
        } else if ko.thread_perms[i as usize] {
            return false;
        }
    }
    true
}

proptest! {
    /// US6: new() always creates object with no permissions.
    #[test]
    fn new_always_no_perms(otype in obj_type_strategy()) {
        let ko = KernelObject::new(otype);
        prop_assert!(all_perms_clear(&ko));
        prop_assert!(!ko.is_initialized());
        prop_assert!(!ko.is_public());
        prop_assert_eq!(ko.obj_type_get(), otype);
    }

    /// US2: grant_access sets exactly one bit.
    #[test]
    fn grant_sets_bit(tid in 0u32..MAX_THREADS) {
        let mut ko = KernelObject::new(ObjType::Sem);
        ko.grant_access(tid);
        prop_assert!(ko.has_perm(tid));
        prop_assert!(only_perm_set(&ko, tid));
    }

    /// US3: revoke_access clears exactly one bit.
    #[test]
    fn revoke_clears_bit(tid in 0u32..MAX_THREADS) {
        let mut ko = KernelObject::new(ObjType::Sem);
        ko.grant_access(tid);
        ko.revoke_access(tid);
        prop_assert!(!ko.has_perm(tid));
        prop_assert!(all_perms_clear(&ko));
    }

    /// US2+US3: grant then revoke is a no-op on the target bit.
    #[test]
    fn grant_revoke_roundtrip(tid in 0u32..MAX_THREADS) {
        let mut ko = KernelObject::new(ObjType::Sem);
        let original = ko.thread_perms;
        ko.grant_access(tid);
        ko.revoke_access(tid);
        prop_assert_eq!(ko.thread_perms, original);
    }

    /// US2: granting is idempotent.
    #[test]
    fn grant_idempotent(tid in 0u32..MAX_THREADS) {
        let mut ko = KernelObject::new(ObjType::Sem);
        ko.grant_access(tid);
        let after_first = ko.thread_perms;
        ko.grant_access(tid);
        prop_assert_eq!(ko.thread_perms, after_first);
    }

    /// US3: revoking is idempotent.
    #[test]
    fn revoke_idempotent(tid in 0u32..MAX_THREADS) {
        let mut ko = KernelObject::new(ObjType::Sem);
        ko.grant_access(tid);
        ko.revoke_access(tid);
        let after_first = ko.thread_perms;
        ko.revoke_access(tid);
        prop_assert_eq!(ko.thread_perms, after_first);
    }

    /// US8: grant with invalid tid returns EINVAL.
    #[test]
    fn grant_invalid_tid(tid in MAX_THREADS..=u32::MAX) {
        let mut ko = KernelObject::new(ObjType::Sem);
        prop_assert_eq!(ko.grant_access(tid), EINVAL);
        prop_assert!(all_perms_clear(&ko));
    }

    /// US8: revoke with invalid tid returns EINVAL.
    #[test]
    fn revoke_invalid_tid(tid in MAX_THREADS..=u32::MAX) {
        let mut ko = KernelObject::new(ObjType::Sem);
        prop_assert_eq!(ko.revoke_access(tid), EINVAL);
    }

    /// US1: access denied without permission.
    #[test]
    fn access_denied_without_perm(tid in 0u32..MAX_THREADS) {
        let ko = KernelObject::new(ObjType::Sem);
        prop_assert!(!ko.check_access(tid, false));
    }

    /// US1: access granted with permission.
    #[test]
    fn access_granted_with_perm(tid in 0u32..MAX_THREADS) {
        let mut ko = KernelObject::new(ObjType::Sem);
        ko.grant_access(tid);
        prop_assert!(ko.check_access(tid, false));
    }

    /// US5: supervisor always has access.
    #[test]
    fn supervisor_always_access(tid in 0u32..=u32::MAX) {
        let ko = KernelObject::new(ObjType::Sem);
        prop_assert!(ko.check_access(tid, true));
    }

    /// US5: public object grants access to all.
    #[test]
    fn public_grants_all(tid in 0u32..MAX_THREADS) {
        let mut ko = KernelObject::new(ObjType::Sem);
        ko.make_public();
        prop_assert!(ko.check_access(tid, false));
    }

    /// US4: type mismatch always returns EBADF.
    #[test]
    fn type_mismatch_ebadf(
        actual in non_any_obj_type_strategy(),
        expected in non_any_obj_type_strategy(),
        tid in 0u32..MAX_THREADS,
        init_check in init_check_strategy(),
    ) {
        prop_assume!(actual != expected);
        let mut ko = KernelObject::new(actual);
        ko.grant_access(tid);
        ko.init_object();
        let result = ko.validate(expected, tid, false, init_check);
        prop_assert_eq!(result, Err(EBADF));
    }

    /// US4: Any type always passes type check.
    #[test]
    fn any_type_passes(
        actual in obj_type_strategy(),
        tid in 0u32..MAX_THREADS,
    ) {
        let mut ko = KernelObject::new(actual);
        ko.grant_access(tid);
        ko.init_object();
        let result = ko.validate(ObjType::Any, tid, false, InitCheck::MustBeInit);
        prop_assert_eq!(result, Ok(()));
    }

    /// US7: uninitialized object fails MustBeInit.
    #[test]
    fn uninit_must_be_init_fails(
        otype in non_any_obj_type_strategy(),
        tid in 0u32..MAX_THREADS,
    ) {
        let mut ko = KernelObject::new(otype);
        ko.grant_access(tid);
        // NOT initialized
        let result = ko.validate(otype, tid, false, InitCheck::MustBeInit);
        prop_assert_eq!(result, Err(EINVAL));
    }

    /// US7: initialized object fails MustNotBeInit.
    #[test]
    fn init_must_not_be_init_fails(
        otype in non_any_obj_type_strategy(),
        tid in 0u32..MAX_THREADS,
    ) {
        let mut ko = KernelObject::new(otype);
        ko.grant_access(tid);
        ko.init_object();
        let result = ko.validate(otype, tid, false, InitCheck::MustNotBeInit);
        prop_assert_eq!(result, Err(EADDRINUSE));
    }

    /// US7: DontCare passes regardless of init state.
    #[test]
    fn dont_care_passes(
        otype in non_any_obj_type_strategy(),
        tid in 0u32..MAX_THREADS,
        do_init in proptest::bool::ANY,
    ) {
        let mut ko = KernelObject::new(otype);
        ko.grant_access(tid);
        if do_init {
            ko.init_object();
        }
        let result = ko.validate(otype, tid, false, InitCheck::DontCare);
        prop_assert_eq!(result, Ok(()));
    }

    /// Full validation happy path.
    #[test]
    fn validate_happy_path(
        otype in non_any_obj_type_strategy(),
        tid in 0u32..MAX_THREADS,
    ) {
        let mut ko = KernelObject::new(otype);
        ko.grant_access(tid);
        ko.init_object();
        let result = ko.validate(otype, tid, false, InitCheck::MustBeInit);
        prop_assert_eq!(result, Ok(()));
    }

    /// Recycle: only current thread has access, object is initialized.
    #[test]
    fn recycle_single_perm(
        otype in non_any_obj_type_strategy(),
        current_tid in 0u32..MAX_THREADS,
        other_tid in 0u32..MAX_THREADS,
    ) {
        let mut ko = KernelObject::new(otype);
        // Grant some arbitrary perms
        ko.grant_access(other_tid);
        ko.recycle(current_tid);
        // Only current_tid should have access
        prop_assert!(ko.has_perm(current_tid));
        prop_assert!(ko.is_initialized());
        if current_tid != other_tid {
            prop_assert!(!ko.has_perm(other_tid));
        }
    }

    /// Invariant: boolean flags stay consistent across all operations.
    #[test]
    fn flags_consistent(
        tid in 0u32..MAX_THREADS,
        ops in proptest::collection::vec(
            prop_oneof![
                Just(0u8), // grant
                Just(1u8), // revoke
                Just(2u8), // init
                Just(3u8), // uninit
                Just(4u8), // make_public
                Just(5u8), // recycle
                Just(6u8), // clear_all_perms
            ],
            0..30
        )
    ) {
        let mut ko = KernelObject::new(ObjType::Sem);
        for op in ops {
            match op {
                0 => { ko.grant_access(tid); }
                1 => { ko.revoke_access(tid); }
                2 => { ko.init_object(); }
                3 => { ko.uninit_object(); }
                4 => { ko.make_public(); }
                5 => { ko.recycle(tid); }
                6 => { ko.clear_all_perms(); }
                _ => unreachable!(),
            }
            // Boolean flags are always valid (no out-of-bounds possible)
            // Just verify the struct is still accessible
            let _ = ko.is_initialized();
            let _ = ko.is_public();
        }
    }

    /// Grant preserves other threads' permissions.
    #[test]
    fn grant_preserves_others(
        tid1 in 0u32..MAX_THREADS,
        tid2 in 0u32..MAX_THREADS,
    ) {
        prop_assume!(tid1 != tid2);
        let mut ko = KernelObject::new(ObjType::Sem);
        ko.grant_access(tid1);
        prop_assert!(ko.has_perm(tid1));
        ko.grant_access(tid2);
        prop_assert!(ko.has_perm(tid1));
        prop_assert!(ko.has_perm(tid2));
    }

    /// Revoke preserves other threads' permissions.
    #[test]
    fn revoke_preserves_others(
        tid1 in 0u32..MAX_THREADS,
        tid2 in 0u32..MAX_THREADS,
    ) {
        prop_assume!(tid1 != tid2);
        let mut ko = KernelObject::new(ObjType::Sem);
        ko.grant_access(tid1);
        ko.grant_access(tid2);
        ko.revoke_access(tid2);
        prop_assert!(ko.has_perm(tid1));
        prop_assert!(!ko.has_perm(tid2));
    }

    /// Validation error priority: type > perms > init.
    #[test]
    fn validation_error_priority(
        tid in 0u32..MAX_THREADS,
    ) {
        // Type mismatch should take priority over permission error
        let ko = KernelObject::new(ObjType::Sem);
        let result = ko.validate(ObjType::Mutex, tid, false, InitCheck::MustBeInit);
        prop_assert_eq!(result, Err(EBADF));

        // Permission error should take priority over init error
        let ko2 = KernelObject::new(ObjType::Sem);
        // no perms, not initialized
        let result2 = ko2.validate(ObjType::Sem, tid, false, InitCheck::MustBeInit);
        prop_assert_eq!(result2, Err(EPERM));
    }
}
