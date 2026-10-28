//! Verified userspace syscall validation model for Zephyr RTOS.
//!
//! This is a formally verified port of zephyr/kernel/userspace.c +
//! userspace_handler.c (1128 lines combined).
//! All safety-critical properties are proven with Verus (SMT/Z3).
//!
//! This module models the **kernel object permission system** of Zephyr's
//! userspace subsystem. Dynamic object allocation, gperf lookup tables,
//! and memory copy helpers remain in C — only the permission bitmask
//! arithmetic, object type validation, and initialization flag logic
//! cross the FFI boundary.
//!
//! Source mapping:
//!   k_object_validate        -> KernelObject::validate         (userspace.c:754-785)
//!   k_thread_perms_set       -> KernelObject::grant_access     (userspace.c:635-642)
//!   k_thread_perms_clear     -> KernelObject::revoke_access    (userspace.c:644-652)
//!   k_thread_perms_all_clear -> KernelObject::clear_all_perms  (userspace.c:661-668)
//!   thread_perms_test        -> KernelObject::check_access     (userspace.c:670-683)
//!   k_object_init            -> KernelObject::init_object      (userspace.c:787-810)
//!   k_object_uninit          -> KernelObject::uninit_object    (userspace.c:823-834)
//!   k_object_recycle         -> KernelObject::recycle          (userspace.c:812-821)
//!   k_object_access_all_grant -> KernelObject::make_public     (userspace.c:745-752)
//!   validate_kernel_object   -> KernelObject::validate         (userspace_handler.c:12-33)
//!   z_vrfy_k_object_access_grant -> (grant_access)            (userspace_handler.c:56-66)
//!   z_vrfy_k_object_release -> (revoke_access)                (userspace_handler.c:69-76)
//!
//! Omitted (not safety-relevant for permission model):
//!   - CONFIG_DYNAMIC_OBJECTS — dynamic allocation/free, gperf tables
//!   - thread_index_get — architecture-specific thread ID mapping
//!   - k_object_find — gperf-based object lookup
//!   - k_object_dump_error — logging/debug
//!   - k_usermode_alloc_from_copy / user_copy — memory copy helpers
//!   - k_thread_perms_inherit — permission inheritance (wordlist iteration)
//!   - otype_to_str — debug string conversion
//!   - Spinlock serialization (obj_lock) — modeled as sequential
//!
//! ASIL-D verified properties:
//!   US1: object access requires permission bit set for calling thread
//!   US2: grant_access sets the permission bit
//!   US3: revoke_access clears the permission bit
//!   US4: object type validation (type must match expected type for syscall)
//!   US5: supervisor mode bypasses permission checks
//!   US6: no permission bits set for uninitialized objects (after new())
//!   US7: K_OBJ_FLAG_INITIALIZED required for access (when init_check == MustBeInit)
//!   US8: thread ID must be valid (< MAX_THREADS)
//!
//! ## Verus modeling note
//!
//! Bitwise operations (`&`, `|`, `~`, `>>`, `<<`) are poorly supported by
//! Z3 in Verus.  This module therefore models flags as individual `bool`
//! fields and the permission bitmask as `[bool; 64]` at exec level,
//! avoiding all bitwise arithmetic.

use vstd::prelude::*;
use crate::error::*;

verus! {

/// Maximum number of threads whose permissions can be tracked.
/// Corresponds to CONFIG_MAX_THREAD_BYTES * 8 in Zephyr.
/// We use 64 bits (8 bytes), matching the common Zephyr default.
pub const MAX_THREADS: u32 = 64;

// ======================================================================
// Object type enumeration
// ======================================================================

/// Kernel object type — identifies what kind of kernel object this is.
///
/// Corresponds to Zephyr's enum k_objects (kobj-types-enum.h, generated).
/// We model the core kernel objects relevant to syscall validation.
/// K_OBJ_ANY (0) is used as a wildcard that matches any type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjType {
    /// Wildcard — matches any object type (K_OBJ_ANY).
    Any,
    /// Thread object (K_OBJ_THREAD).
    Thread,
    /// Semaphore (K_OBJ_SEM).
    Sem,
    /// Mutex (K_OBJ_MUTEX).
    Mutex,
    /// Condition variable (K_OBJ_CONDVAR).
    CondVar,
    /// Message queue (K_OBJ_MSGQ).
    MsgQ,
    /// Stack (K_OBJ_STACK).
    Stack,
    /// Pipe (K_OBJ_PIPE).
    Pipe,
    /// Timer (K_OBJ_TIMER).
    Timer,
    /// Event (K_OBJ_EVENT).
    Event,
    /// Memory slab (K_OBJ_MEM_SLAB).
    MemSlab,
    /// FIFO (K_OBJ_FIFO — alias for queue).
    Fifo,
    /// LIFO (K_OBJ_LIFO — alias for queue).
    Lifo,
    /// Mutex (sys_mutex — K_OBJ_SYS_MUTEX).
    SysMutex,
    /// Futex (K_OBJ_FUTEX).
    Futex,
    /// Mailbox (K_OBJ_MBOX).
    Mbox,
}

// ======================================================================
// Initialization check mode
// ======================================================================

/// Corresponds to Zephyr's enum _obj_init_check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitCheck {
    /// Object MUST be initialized (_OBJ_INIT_TRUE = 0).
    MustBeInit,
    /// Object MUST NOT be initialized (_OBJ_INIT_FALSE = -1).
    MustNotBeInit,
    /// Don't care about initialization state (_OBJ_INIT_ANY = 1).
    DontCare,
}

// ======================================================================
// Kernel object
// ======================================================================

/// A kernel object with type, flags, and per-thread permission array.
///
/// Corresponds to Zephyr's struct k_object.
///
/// We model perms as `[bool; 64]` — entry N true means thread N has
/// access.  This avoids bitwise arithmetic that Z3 cannot handle.
///
/// Flags are modeled as individual `bool` fields to avoid bitwise masking.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KernelObject {
    /// Object type (which kernel primitive this is).
    pub obj_type: ObjType,
    /// K_OBJ_FLAG_INITIALIZED — object has been initialized.
    pub flag_initialized: bool,
    /// K_OBJ_FLAG_PUBLIC — object is accessible to all threads.
    pub flag_public: bool,
    /// K_OBJ_FLAG_ALLOC — object was dynamically allocated.
    pub flag_alloc: bool,
    /// K_OBJ_FLAG_DRIVER — object is a device driver.
    pub flag_driver: bool,
    /// Per-thread permission array (index N = thread N has access).
    pub thread_perms: [bool; 64],
}

impl KernelObject {

    // ==================================================================
    // Specification predicates
    // ==================================================================

    /// Structural invariant — always maintained (trivially true for bool model).
    pub open spec fn inv(&self) -> bool {
        true
    }

    /// Check if the INITIALIZED flag is set (spec).
    pub open spec fn is_initialized_spec(&self) -> bool {
        self.flag_initialized
    }

    /// Check if the PUBLIC flag is set (spec).
    pub open spec fn is_public_spec(&self) -> bool {
        self.flag_public
    }

    /// Check if thread `tid` has permission (spec).
    ///
    /// US1: access requires the permission bit set.
    pub open spec fn has_perm_spec(&self, tid: u32) -> bool {
        tid < MAX_THREADS && self.thread_perms[tid as int]
    }

    /// Check if any permission is set (spec).
    pub open spec fn has_any_perm_spec(&self) -> bool {
        exists|i: int| 0 <= i < MAX_THREADS as int && self.thread_perms[i]
    }

    /// Spec-level access check (mirrors check_access logic).
    pub open spec fn check_access_spec(&self, tid: u32, is_supervisor: bool) -> bool {
        is_supervisor
        || self.is_public_spec()
        || (tid < MAX_THREADS && self.has_perm_spec(tid))
    }

    /// Spec helper: all permissions false.
    pub open spec fn all_perms_clear_spec(&self) -> bool {
        forall|i: int| 0 <= i < MAX_THREADS as int ==> !self.thread_perms[i]
    }

    // ------------------------------------------------------------------
    // Operation contracts as spec fns (gale#462). Each exec op below
    // `ensures` exactly its `*_post`, and the lemmas at the bottom `require`
    // the same `*_post` -- one definition, so a lemma cannot drift from the
    // contract it reasons about. Before this, those lemmas were `ensures true`
    // with the property written as comments.
    // ------------------------------------------------------------------

    /// What grant_access guarantees about the state it leaves (US2, US8).
    pub open spec fn grant_post(pre: KernelObject, post: KernelObject, tid: u32, rc: i32) -> bool {
        &&& tid >= MAX_THREADS ==> {
            &&& rc == EINVAL
            &&& post.thread_perms === pre.thread_perms
            &&& post.flag_initialized == pre.flag_initialized
            &&& post.flag_public == pre.flag_public
            &&& post.obj_type === pre.obj_type
        }
        &&& tid < MAX_THREADS ==> {
            &&& rc == OK
            &&& post.thread_perms[tid as int]
            &&& post.flag_initialized == pre.flag_initialized
            &&& post.flag_public == pre.flag_public
            &&& post.obj_type === pre.obj_type
            &&& forall|i: int| 0 <= i < MAX_THREADS as int && i != tid as int
                ==> post.thread_perms[i] == pre.thread_perms[i]
        }
    }

    /// What revoke_access guarantees about the state it leaves (US3, US8).
    pub open spec fn revoke_post(pre: KernelObject, post: KernelObject, tid: u32, rc: i32) -> bool {
        &&& tid >= MAX_THREADS ==> {
            &&& rc == EINVAL
            &&& post.thread_perms === pre.thread_perms
            &&& post.flag_initialized == pre.flag_initialized
            &&& post.flag_public == pre.flag_public
            &&& post.obj_type === pre.obj_type
        }
        &&& tid < MAX_THREADS ==> {
            &&& rc == OK
            &&& !post.thread_perms[tid as int]
            &&& post.flag_initialized == pre.flag_initialized
            &&& post.flag_public == pre.flag_public
            &&& post.obj_type === pre.obj_type
            &&& forall|i: int| 0 <= i < MAX_THREADS as int && i != tid as int
                ==> post.thread_perms[i] == pre.thread_perms[i]
        }
    }

    /// What recycle guarantees: exactly `tid` holds a permission, and the
    /// object is initialized.
    pub open spec fn recycle_post(pre: KernelObject, post: KernelObject, tid: u32, rc: i32) -> bool {
        &&& rc == OK
        &&& post.thread_perms[tid as int]
        &&& forall|i: int| 0 <= i < MAX_THREADS as int && i != tid as int ==> !post.thread_perms[i]
        &&& post.is_initialized_spec()
        &&& post.obj_type === pre.obj_type
    }

    /// The full result of validate (US1, US4, US5, US7). validate's ensures
    /// used to constrain only the `Ok` case, so WHICH error a failure returns
    /// -- e.g. US7's EINVAL for an uninitialized MustBeInit object -- was not
    /// proven by anything.
    pub open spec fn validate_spec(
        &self,
        expected_type: ObjType,
        tid: u32,
        is_supervisor: bool,
        init_check: InitCheck,
    ) -> Result<(), i32> {
        if !(expected_type === ObjType::Any || self.obj_type === expected_type) {
            Err(EBADF)
        } else if !self.check_access_spec(tid, is_supervisor) {
            Err(EPERM)
        } else {
            match init_check {
                InitCheck::MustBeInit => if !self.flag_initialized { Err(EINVAL) } else { Ok(()) },
                InitCheck::MustNotBeInit => if self.flag_initialized { Err(EADDRINUSE) } else { Ok(()) },
                InitCheck::DontCare => Ok(()),
            }
        }
    }

    // ==================================================================
    // Constructor
    // ==================================================================

    /// Create a new uninitialized kernel object with no permissions.
    ///
    /// US6: no permission bits set for uninitialized objects.
    ///
    /// In Zephyr, objects are created by the gperf table generator with
    /// all perms zeroed. Initialization happens via k_object_init().
    pub fn new(obj_type: ObjType) -> (result: KernelObject)
        ensures
            result.inv(),
            result.obj_type === obj_type,
            // US6: no permission bits set
            result.all_perms_clear_spec(),
            !result.is_initialized_spec(),
            !result.is_public_spec(),
            !result.flag_alloc,
            !result.flag_driver,
    {
        KernelObject {
            obj_type,
            flag_initialized: false,
            flag_public: false,
            flag_alloc: false,
            flag_driver: false,
            thread_perms: [false; 64],
        }
    }

    // ==================================================================
    // Permission operations
    // ==================================================================

    /// Grant a thread access to this object.
    ///
    /// Models k_thread_perms_set() (userspace.c:635-642):
    ///   sys_bitfield_set_bit(&ko->perms, index);
    ///
    /// US2: grant_access sets the permission bit.
    /// US8: thread ID must be valid (< MAX_THREADS).
    pub fn grant_access(&mut self, tid: u32) -> (rc: i32)
        requires
            old(self).inv(),
        ensures
            self.inv(),
            Self::grant_post(*old(self), *self, tid, rc),
            tid < MAX_THREADS ==> self.has_perm_spec(tid),
    {
        if tid >= MAX_THREADS {
            return EINVAL;
        }
        self.thread_perms[tid as usize] = true;
        OK
    }

    /// Revoke a thread's access to this object.
    ///
    /// Models k_thread_perms_clear() (userspace.c:644-652):
    ///   sys_bitfield_clear_bit(&ko->perms, index);
    ///
    /// US3: revoke_access clears the permission bit.
    /// US8: thread ID must be valid (< MAX_THREADS).
    pub fn revoke_access(&mut self, tid: u32) -> (rc: i32)
        requires
            old(self).inv(),
        ensures
            self.inv(),
            Self::revoke_post(*old(self), *self, tid, rc),
            tid < MAX_THREADS ==> !self.has_perm_spec(tid),
    {
        if tid >= MAX_THREADS {
            return EINVAL;
        }
        self.thread_perms[tid as usize] = false;
        OK
    }

    /// Clear all permission bits for a specific thread across this object.
    ///
    /// Models the per-object part of k_thread_perms_all_clear()
    /// (userspace.c:654-668). The full function iterates all objects;
    /// we model clearing one thread's bit from a single object.
    pub fn clear_thread_perm(&mut self, tid: u32) -> (rc: i32)
        requires
            old(self).inv(),
        ensures
            self.inv(),
            tid >= MAX_THREADS ==> {
                &&& rc == EINVAL
                &&& self.thread_perms === old(self).thread_perms
            },
            tid < MAX_THREADS ==> {
                &&& rc == OK
                &&& !self.thread_perms[tid as int]
                &&& !self.has_perm_spec(tid)
            },
            self.flag_initialized == old(self).flag_initialized,
            self.flag_public == old(self).flag_public,
            self.obj_type === old(self).obj_type,
    {
        self.revoke_access(tid)
    }

    /// Clear all permission bits (reset bitmask to zero).
    ///
    /// Models memset(ko->perms, 0, sizeof(ko->perms)) used in
    /// k_object_recycle() (userspace.c:817).
    pub fn clear_all_perms(&mut self)
        requires
            old(self).inv(),
        ensures
            self.inv(),
            self.all_perms_clear_spec(),
            self.flag_initialized == old(self).flag_initialized,
            self.flag_public == old(self).flag_public,
            self.obj_type === old(self).obj_type,
    {
        self.thread_perms = [false; 64];
    }

    // ==================================================================
    // Access check (thread_perms_test + validate)
    // ==================================================================

    /// Check if a thread has access to this object.
    ///
    /// Models thread_perms_test() (userspace.c:670-683):
    ///   if (ko->flags & K_OBJ_FLAG_PUBLIC) return 1;
    ///   return sys_bitfield_test_bit(&ko->perms, index);
    ///
    /// US1: access requires permission bit set for calling thread.
    /// US5: public flag bypasses permission check (supervisor mode
    ///       is modeled by the `is_supervisor` parameter).
    /// US8: invalid tid always fails (unless public or supervisor).
    pub fn check_access(&self, tid: u32, is_supervisor: bool) -> (granted: bool)
        requires
            self.inv(),
        ensures
            // The exec check IS the spec check (gale#462) -- so lemmas about
            // check_access_spec are lemmas about this function.
            granted == self.check_access_spec(tid, is_supervisor),
            // US5: supervisor always has access
            is_supervisor ==> granted,
            // US1: public objects grant access to everyone
            !is_supervisor && self.is_public_spec() ==> granted,
            // US1: non-public, non-supervisor -> need permission bit
            !is_supervisor && !self.is_public_spec() && tid < MAX_THREADS ==>
                (granted == self.has_perm_spec(tid)),
            // US8: invalid tid with non-public, non-supervisor -> denied
            !is_supervisor && !self.is_public_spec() && tid >= MAX_THREADS ==>
                !granted,
    {
        // US5: supervisor mode bypasses all checks
        if is_supervisor {
            return true;
        }

        // Public objects are accessible to all threads
        if self.flag_public {
            return true;
        }

        // US8: invalid thread ID
        if tid >= MAX_THREADS {
            return false;
        }

        // US1: check per-thread permission
        self.thread_perms[tid as usize]
    }

    // ==================================================================
    // Object validation (k_object_validate)
    // ==================================================================

    /// Validate a kernel object for a syscall.
    ///
    /// Models k_object_validate() (userspace.c:754-785):
    ///   1. Type check (US4): if otype != K_OBJ_ANY, ko->type must match
    ///   2. Permission check (US1): thread must have access
    ///   3. Initialization check (US7):
    ///      - MustBeInit: K_OBJ_FLAG_INITIALIZED must be set
    ///      - MustNotBeInit: K_OBJ_FLAG_INITIALIZED must NOT be set
    ///      - DontCare: skip check
    ///
    /// Returns:
    ///   Ok(())     — validation passed
    ///   Err(EBADF) — type mismatch (US4)
    ///   Err(EPERM) — no permission (US1)
    ///   Err(EINVAL)     — not initialized when required (US7)
    ///   Err(EADDRINUSE) — already initialized when must-not-be (US7)
    pub fn validate(
        &self,
        expected_type: ObjType,
        tid: u32,
        is_supervisor: bool,
        init_check: InitCheck,
    ) -> (result: Result<(), i32>)
        requires
            self.inv(),
        ensures
            // Full result, not only the Ok case (gale#462).
            result === self.validate_spec(expected_type, tid, is_supervisor, init_check),
            // US4: type mismatch -> EBADF
            expected_type !== ObjType::Any && self.obj_type !== expected_type ==> {
                result.is_err() && result === Err::<(), i32>(EBADF)
            },
            // When type matches (or Any), and has access, and init check passes -> Ok
            result.is_ok() ==> {
                // US4: type must match or be Any
                &&& (expected_type === ObjType::Any || self.obj_type === expected_type)
                // US1/US5: must have access
                &&& self.check_access_spec(tid, is_supervisor)
                // US7: initialization state matches
                &&& (init_check === InitCheck::MustBeInit ==> self.is_initialized_spec())
                &&& (init_check === InitCheck::MustNotBeInit ==> !self.is_initialized_spec())
            },
    {
        // US4: type validation — use match to avoid PartialEq on enums
        let type_ok = match expected_type {
            ObjType::Any => true,
            ObjType::Thread => matches!(self.obj_type, ObjType::Thread),
            ObjType::Sem => matches!(self.obj_type, ObjType::Sem),
            ObjType::Mutex => matches!(self.obj_type, ObjType::Mutex),
            ObjType::CondVar => matches!(self.obj_type, ObjType::CondVar),
            ObjType::MsgQ => matches!(self.obj_type, ObjType::MsgQ),
            ObjType::Stack => matches!(self.obj_type, ObjType::Stack),
            ObjType::Pipe => matches!(self.obj_type, ObjType::Pipe),
            ObjType::Timer => matches!(self.obj_type, ObjType::Timer),
            ObjType::Event => matches!(self.obj_type, ObjType::Event),
            ObjType::MemSlab => matches!(self.obj_type, ObjType::MemSlab),
            ObjType::Fifo => matches!(self.obj_type, ObjType::Fifo),
            ObjType::Lifo => matches!(self.obj_type, ObjType::Lifo),
            ObjType::SysMutex => matches!(self.obj_type, ObjType::SysMutex),
            ObjType::Futex => matches!(self.obj_type, ObjType::Futex),
            ObjType::Mbox => matches!(self.obj_type, ObjType::Mbox),
        };

        if !type_ok {
            return Err(EBADF);
        }

        // US1/US5: permission check
        if !self.check_access(tid, is_supervisor) {
            return Err(EPERM);
        }

        // US7: initialization state check
        match init_check {
            InitCheck::MustBeInit => {
                if !self.flag_initialized {
                    return Err(EINVAL);
                }
            }
            InitCheck::MustNotBeInit => {
                if self.flag_initialized {
                    return Err(EADDRINUSE);
                }
            }
            InitCheck::DontCare => { /* skip */ }
        }

        Ok(())
    }

    // ==================================================================
    // Initialization management
    // ==================================================================

    /// Mark the object as initialized.
    ///
    /// Models k_object_init() (userspace.c:787-810):
    ///   ko->flags |= K_OBJ_FLAG_INITIALIZED;
    pub fn init_object(&mut self)
        requires
            old(self).inv(),
        ensures
            self.inv(),
            self.is_initialized_spec(),
            self.flag_initialized,
            self.flag_public == old(self).flag_public,
            self.flag_alloc == old(self).flag_alloc,
            self.flag_driver == old(self).flag_driver,
            self.thread_perms === old(self).thread_perms,
            self.obj_type === old(self).obj_type,
    {
        self.flag_initialized = true;
    }

    /// Mark the object as uninitialized.
    ///
    /// Models k_object_uninit() (userspace.c:823-834):
    ///   ko->flags &= ~K_OBJ_FLAG_INITIALIZED;
    pub fn uninit_object(&mut self)
        requires
            old(self).inv(),
        ensures
            self.inv(),
            !self.is_initialized_spec(),
            !self.flag_initialized,
            self.flag_public == old(self).flag_public,
            self.flag_alloc == old(self).flag_alloc,
            self.flag_driver == old(self).flag_driver,
            self.thread_perms === old(self).thread_perms,
            self.obj_type === old(self).obj_type,
    {
        self.flag_initialized = false;
    }

    /// Recycle the object: clear all permissions, grant to current thread,
    /// and mark as initialized.
    ///
    /// Models k_object_recycle() (userspace.c:812-821):
    ///   memset(ko->perms, 0, sizeof(ko->perms));
    ///   k_thread_perms_set(ko, _current);
    ///   ko->flags |= K_OBJ_FLAG_INITIALIZED;
    pub fn recycle(&mut self, current_tid: u32) -> (rc: i32)
        requires
            old(self).inv(),
            current_tid < MAX_THREADS,
        ensures
            self.inv(),
            Self::recycle_post(*old(self), *self, current_tid, rc),
            self.has_perm_spec(current_tid),
    {
        self.thread_perms = [false; 64];
        self.thread_perms[current_tid as usize] = true;
        self.flag_initialized = true;
        OK
    }

    /// Make the object accessible to all threads (public).
    ///
    /// Models k_object_access_all_grant() (userspace.c:745-752):
    ///   ko->flags |= K_OBJ_FLAG_PUBLIC;
    pub fn make_public(&mut self)
        requires
            old(self).inv(),
        ensures
            self.inv(),
            self.is_public_spec(),
            self.flag_public,
            self.flag_initialized == old(self).flag_initialized,
            self.flag_alloc == old(self).flag_alloc,
            self.flag_driver == old(self).flag_driver,
            self.thread_perms === old(self).thread_perms,
            self.obj_type === old(self).obj_type,
    {
        self.flag_public = true;
    }

    // ==================================================================
    // Query operations
    // ==================================================================

    /// Check if the object is initialized.
    pub fn is_initialized(&self) -> (r: bool)
        requires self.inv(),
        ensures r == self.is_initialized_spec(),
    {
        self.flag_initialized
    }

    /// Check if the object is public.
    pub fn is_public(&self) -> (r: bool)
        requires self.inv(),
        ensures r == self.is_public_spec(),
    {
        self.flag_public
    }

    /// Check if a specific thread has permission.
    pub fn has_perm(&self, tid: u32) -> (r: bool)
        requires
            self.inv(),
            tid < MAX_THREADS,
        ensures
            r == self.has_perm_spec(tid),
    {
        self.thread_perms[tid as usize]
    }

    /// Get the object type.
    pub fn obj_type_get(&self) -> (r: ObjType)
        ensures r === self.obj_type,
    {
        self.obj_type
    }
}

// ======================================================================
// Compositional proofs
// ======================================================================

// There is no lemma_invariant_inductive here (gale#462). KernelObject::inv()
// is `true` by definition -- the bool-array model has no structural invariant
// to preserve -- so a P1 lemma would restate `true`. The lemma that stood here
// had `ensures true` and an empty body; it was removed rather than dressed up.

/// US2+US3: grant then revoke restores tid's original (false) permission and
/// leaves every other thread's permission as it was.
pub proof fn lemma_grant_revoke_roundtrip(
    a: KernelObject, b: KernelObject, c: KernelObject, tid: u32, r1: i32, r2: i32,
)
    requires
        tid < MAX_THREADS,
        !a.thread_perms[tid as int],
        KernelObject::grant_post(a, b, tid, r1),
        KernelObject::revoke_post(b, c, tid, r2),
    ensures
        forall|i: int| 0 <= i < MAX_THREADS as int ==> c.thread_perms[i] == a.thread_perms[i],
        c.flag_initialized == a.flag_initialized,
        c.flag_public == a.flag_public,
        c.obj_type === a.obj_type,
{
}

/// US2: grant_access is idempotent -- a second grant of the same tid leaves
/// the object exactly as the first grant did.
pub proof fn lemma_grant_idempotent(
    a: KernelObject, b: KernelObject, c: KernelObject, tid: u32, r1: i32, r2: i32,
)
    requires
        KernelObject::grant_post(a, b, tid, r1),
        KernelObject::grant_post(b, c, tid, r2),
    ensures
        forall|i: int| 0 <= i < MAX_THREADS as int ==> c.thread_perms[i] == b.thread_perms[i],
        c.flag_initialized == b.flag_initialized,
        c.flag_public == b.flag_public,
        c.obj_type === b.obj_type,
        r2 == r1,
{
}

/// US3: revoke_access is idempotent.
pub proof fn lemma_revoke_idempotent(
    a: KernelObject, b: KernelObject, c: KernelObject, tid: u32, r1: i32, r2: i32,
)
    requires
        KernelObject::revoke_post(a, b, tid, r1),
        KernelObject::revoke_post(b, c, tid, r2),
    ensures
        forall|i: int| 0 <= i < MAX_THREADS as int ==> c.thread_perms[i] == b.thread_perms[i],
        c.flag_initialized == b.flag_initialized,
        c.flag_public == b.flag_public,
        c.obj_type === b.obj_type,
        r2 == r1,
{
}

/// US5: supervisor mode is granted access to any object, whatever its
/// permissions. check_access's ensures equates it with check_access_spec, so
/// this is a statement about the shipped check.
pub proof fn lemma_supervisor_always_granted(ko: KernelObject, tid: u32)
    ensures
        ko.check_access_spec(tid, true),
{
}

/// US6: an object in the state new() produces -- no permissions, not public --
/// denies every non-supervisor thread.
pub proof fn lemma_new_no_perms(ko: KernelObject, tid: u32)
    requires
        ko.all_perms_clear_spec(),
        !ko.is_public_spec(),
    ensures
        !ko.has_perm_spec(tid),
        !ko.check_access_spec(tid, false),
{
}

/// US4: a type mismatch fails validation with EBADF, before any permission or
/// initialization check, for every caller.
pub proof fn lemma_type_mismatch_fails(
    ko: KernelObject, expected: ObjType, tid: u32, is_supervisor: bool, init_check: InitCheck,
)
    requires
        expected !== ObjType::Any,
        ko.obj_type !== expected,
    ensures
        ko.validate_spec(expected, tid, is_supervisor, init_check) === Err::<(), i32>(EBADF),
{
}

/// US7: an uninitialized object fails MustBeInit validation with EINVAL. A
/// supervisor bypasses the permission check but not the initialization check.
pub proof fn lemma_uninit_fails_must_be_init(ko: KernelObject, expected: ObjType, tid: u32)
    requires
        !ko.is_initialized_spec(),
        expected === ObjType::Any || ko.obj_type === expected,
    ensures
        ko.validate_spec(expected, tid, true, InitCheck::MustBeInit) === Err::<(), i32>(EINVAL),
{
}

/// US2+US1: after a successful grant, that thread passes the access check.
pub proof fn lemma_grant_then_check(a: KernelObject, b: KernelObject, tid: u32, rc: i32)
    requires
        tid < MAX_THREADS,
        KernelObject::grant_post(a, b, tid, rc),
    ensures
        b.has_perm_spec(tid),
        b.check_access_spec(tid, false),
{
}

/// US3+US1: after a revoke, that thread fails the access check on a
/// non-public object (revoke does not change publicity).
pub proof fn lemma_revoke_then_check(a: KernelObject, b: KernelObject, tid: u32, rc: i32)
    requires
        tid < MAX_THREADS,
        !a.is_public_spec(),
        KernelObject::revoke_post(a, b, tid, rc),
    ensures
        !b.has_perm_spec(tid),
        !b.check_access_spec(tid, false),
{
}

/// recycle leaves exactly one thread with access: the recycling thread.
pub proof fn lemma_recycle_single_perm(a: KernelObject, b: KernelObject, tid: u32, other: u32, rc: i32)
    requires
        tid < MAX_THREADS,
        other != tid,
        KernelObject::recycle_post(a, b, tid, rc),
    ensures
        b.has_perm_spec(tid),
        !b.has_perm_spec(other),
{
}

// ======================================================================
// Standalone decide functions for FFI
// ======================================================================

/// Decision for access check: check if a thread has access to an object.
///
/// US1: permission required. US5: public bypasses.
/// Returns true if access granted.
pub fn access_decide(is_public: bool, has_perm_bit: bool) -> (result: bool)
    ensures
        result == (is_public || has_perm_bit),
{
    is_public || has_perm_bit
}

/// Decision for object validation: type check, access check, init check.
///
/// US1: permission. US4: type match. US5: supervisor. US7: init state.
/// Returns Ok(()) on success, Err(code) on failure.
/// init_check: 0=MustBeInit, -1=MustNotBeInit, 1=DontCare
pub fn validate_decide(
    type_matches: bool,
    has_access: bool,
    is_initialized: bool,
    init_check: i8,
) -> (result: Result<(), i32>)
    ensures
        !type_matches ==> result === Err::<(), i32>(EBADF),
        (type_matches && !has_access) ==> result === Err::<(), i32>(EPERM),
{
    if !type_matches {
        return Err(EBADF);
    }
    if !has_access {
        return Err(EPERM);
    }
    // init_check: 0 = MustBeInit, -1 = MustNotBeInit, other = DontCare
    if init_check == 0 {
        if !is_initialized {
            return Err(EINVAL);
        }
    } else if init_check == -1 {
        if is_initialized {
            return Err(EADDRINUSE);
        }
    }
    Ok(())
}

// ======================================================================
// Flag constants — mirror <zephyr/sys/kobject.h> bit layout.
// ======================================================================

/// K_OBJ_FLAG_INITIALIZED — object has been initialized.
pub const FLAG_INITIALIZED: u8 = 0x01;
/// K_OBJ_FLAG_PUBLIC — object is accessible to all threads.
pub const FLAG_PUBLIC: u8 = 0x02;

/// Decide new flags for k_object_init (userspace.c:787-810).
///
/// `ko->flags |= K_OBJ_FLAG_INITIALIZED;`
///
/// US7: init flag management — the INITIALIZED bit is set, all other
/// bits are preserved.
pub fn init_flags_decide(current_flags: u8) -> (result: u8)
    ensures
        result == (current_flags | FLAG_INITIALIZED),
        // US7a: INITIALIZED bit is always set in the result.
        (result & FLAG_INITIALIZED) == FLAG_INITIALIZED,
        // Monotonic: bits that were set remain set.
        (current_flags & result) == current_flags,
{
    let result = current_flags | FLAG_INITIALIZED;
    assert((result & FLAG_INITIALIZED) == FLAG_INITIALIZED) by (bit_vector)
        requires result == (current_flags | FLAG_INITIALIZED);
    assert((current_flags & result) == current_flags) by (bit_vector)
        requires result == (current_flags | FLAG_INITIALIZED);
    result
}

/// Decide new flags for k_object_uninit (userspace.c:823-834).
///
/// `ko->flags &= ~K_OBJ_FLAG_INITIALIZED;`
///
/// US7: init flag management — the INITIALIZED bit is cleared, all
/// other bits are preserved.
pub fn uninit_flags_decide(current_flags: u8) -> (result: u8)
    ensures
        result == (current_flags & !FLAG_INITIALIZED),
        // US7b: INITIALIZED bit is cleared in the result.
        (result & FLAG_INITIALIZED) == 0u8,
        // Other bits (e.g. PUBLIC) are preserved.
        (result & FLAG_PUBLIC) == (current_flags & FLAG_PUBLIC),
{
    let result = current_flags & !FLAG_INITIALIZED;
    assert((result & FLAG_INITIALIZED) == 0u8) by (bit_vector)
        requires result == (current_flags & !FLAG_INITIALIZED);
    assert((result & FLAG_PUBLIC) == (current_flags & FLAG_PUBLIC)) by (bit_vector)
        requires result == (current_flags & !FLAG_INITIALIZED);
    result
}

/// Decide new flags for k_object_recycle (userspace.c:812-821).
///
/// ```c
/// memset(ko->perms, 0, sizeof(ko->perms));
/// k_thread_perms_set(ko, _current);
/// ko->flags |= K_OBJ_FLAG_INITIALIZED;
/// ```
///
/// This helper returns only the flag-update portion (the perms memset
/// and grant_access are C-side side effects that cannot be expressed
/// as a scalar decision).
///
/// US2 (grant caller), US6 (clear perms), US7 (init) — the boolean
/// `clear_perms` is always true because recycle always clears and grants.
pub fn recycle_flags_decide(current_flags: u8) -> (result: u8)
    ensures
        result == (current_flags | FLAG_INITIALIZED),
        (result & FLAG_INITIALIZED) == FLAG_INITIALIZED,
{
    let result = current_flags | FLAG_INITIALIZED;
    assert((result & FLAG_INITIALIZED) == FLAG_INITIALIZED) by (bit_vector)
        requires result == (current_flags | FLAG_INITIALIZED);
    result
}

/// Decide new flags for k_object_access_all_grant (userspace.c:745-752).
///
/// `ko->flags |= K_OBJ_FLAG_PUBLIC;`
///
/// US5: public flag grants universal access — the PUBLIC bit is set,
/// all other bits are preserved.
pub fn make_public_flags_decide(current_flags: u8) -> (result: u8)
    ensures
        result == (current_flags | FLAG_PUBLIC),
        // US5: PUBLIC bit is always set in the result.
        (result & FLAG_PUBLIC) == FLAG_PUBLIC,
        // Monotonic: INITIALIZED (and other) bits are preserved.
        (result & FLAG_INITIALIZED) == (current_flags & FLAG_INITIALIZED),
{
    let result = current_flags | FLAG_PUBLIC;
    assert((result & FLAG_PUBLIC) == FLAG_PUBLIC) by (bit_vector)
        requires result == (current_flags | FLAG_PUBLIC);
    assert((result & FLAG_INITIALIZED) == (current_flags & FLAG_INITIALIZED)) by (bit_vector)
        requires result == (current_flags | FLAG_PUBLIC);
    result
}

} // verus!
