/*
 * Gale Userspace FFI — verified permission/type/init validation.
 *
 * These functions replace the safety-critical decision logic in
 * kernel/userspace.c. Object lookup (gperf), spinlock serialization,
 * dynamic objects, memory copy helpers, and debug logging remain
 * native Zephyr C.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#ifndef GALE_USERSPACE_H
#define GALE_USERSPACE_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ---- Decision structs ---- */

/**
 * Decision for k_object_validate — tells C shim whether the object
 * passes type/permission/initialization checks.
 *
 * Verified: US1 (permission required), US4 (type match), US5 (supervisor
 * bypass), US7 (init flag check).
 */
struct gale_userspace_validate_decision {
    int32_t ret;    /* 0=OK, -EBADF/-EPERM/-EINVAL/-EADDRINUSE */
};

struct gale_userspace_validate_decision gale_k_object_validate_decide(
    uint8_t obj_type,       /* ko->type */
    uint8_t expected_type,  /* otype argument (0 = K_OBJ_ANY) */
    uint8_t flags,          /* ko->flags */
    uint8_t has_access,     /* 1 if thread_perms_test() passed */
    int8_t  init_check      /* _OBJ_INIT_TRUE=0, _OBJ_INIT_FALSE=-1, _OBJ_INIT_ANY=1 */
);

/**
 * Decision for thread_perms_test — tells C shim whether a thread
 * has access to a kernel object.
 *
 * Verified: US1 (permission bit required), US5 (public bypass).
 */
struct gale_userspace_access_decision {
    uint8_t granted;    /* 1=access granted, 0=denied */
};

struct gale_userspace_access_decision gale_k_object_access_decide(
    uint8_t flags,          /* ko->flags (checks K_OBJ_FLAG_PUBLIC) */
    uint8_t has_perm_bit    /* 1 if sys_bitfield_test_bit passed */
);

/* ---- Action codes for grant/revoke/init/uninit/recycle ---- */

/**
 * Decision for k_object_init — whether to set the initialized flag.
 *
 * Verified: US7 (initialization flag management).
 */
struct gale_userspace_init_decision {
    uint8_t new_flags;  /* flags | K_OBJ_FLAG_INITIALIZED */
};

struct gale_userspace_init_decision gale_k_object_init_decide(
    uint8_t current_flags
);

/**
 * Decision for k_object_uninit — whether to clear the initialized flag.
 *
 * Verified: US7 (initialization flag management).
 */
struct gale_userspace_uninit_decision {
    uint8_t new_flags;  /* flags & ~K_OBJ_FLAG_INITIALIZED */
};

struct gale_userspace_uninit_decision gale_k_object_uninit_decide(
    uint8_t current_flags
);

/**
 * Decision for k_object_recycle — clear perms, grant to caller, init.
 *
 * Verified: US2 (grant), US6 (clear all perms), US7 (init).
 */
struct gale_userspace_recycle_decision {
    uint8_t new_flags;  /* flags | K_OBJ_FLAG_INITIALIZED */
    uint8_t clear_perms; /* 1 = memset perms to 0, then set caller's bit */
};

struct gale_userspace_recycle_decision gale_k_object_recycle_decide(
    uint8_t current_flags
);

/**
 * Decision for k_object_access_all_grant (make public).
 *
 * Verified: US5 (public flag grants universal access).
 */
struct gale_userspace_public_decision {
    uint8_t new_flags;  /* flags | K_OBJ_FLAG_PUBLIC */
};

struct gale_userspace_public_decision gale_k_object_make_public_decide(
    uint8_t current_flags
);

/* ---- Flag bit constants (must match Zephyr's kobject.h) ---- */

#define GALE_K_OBJ_FLAG_INITIALIZED  0x01
#define GALE_K_OBJ_FLAG_PUBLIC       0x02
#define GALE_K_OBJ_FLAG_ALLOC        0x04
#define GALE_K_OBJ_FLAG_DRIVER       0x08

#ifdef __cplusplus
}
#endif

#endif /* GALE_USERSPACE_H */
