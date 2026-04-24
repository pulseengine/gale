/*
 * Copyright (c) 2017 Intel Corporation
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale userspace — phase 2: Extract->Decide->Apply pattern.
 *
 * This is kernel/userspace.c with the 6 safety-critical validation
 * functions rewritten to use Rust decision structs. All infrastructure
 * (gperf lookup, dynamic objects, memory copy helpers, spinlocks,
 * thread index management, debug logging) remains native Zephyr C.
 *
 * Verified operations (Verus + SMT/Z3 proofs):
 *   gale_k_object_access_decide   — US1 (perm required), US5 (public bypass)
 *   gale_k_object_validate_decide — US1/US4/US5/US7 (type+perm+init)
 *   gale_k_object_init_decide     — US7 (set init flag)
 *   gale_k_object_uninit_decide   — US7 (clear init flag)
 *   gale_k_object_recycle_decide  — US2/US6/US7 (clear+grant+init)
 *   gale_k_object_make_public_decide — US5 (public flag)
 */


#include <zephyr/kernel.h>
#include <string.h>
#include <zephyr/sys/math_extras.h>
#include <zephyr/sys/rb.h>
#include <zephyr/kernel_structs.h>
#include <zephyr/sys/sys_io.h>
#include <ksched.h>
#include <zephyr/syscall.h>
#include <zephyr/internal/syscall_handler.h>
#include <zephyr/device.h>
#include <zephyr/init.h>
#include <stdbool.h>
#include <zephyr/app_memory/app_memdomain.h>
#include <zephyr/sys/libc-hooks.h>
#include <zephyr/sys/mutex.h>
#include <zephyr/sys/util.h>
#include <inttypes.h>
#include <zephyr/linker/linker-defs.h>

#include "gale_userspace.h"
#include "gale_error_check.h"  /* STPA GAP-6: compile-time errno sync */

#ifdef Z_LIBC_PARTITION_EXISTS
K_APPMEM_PARTITION_DEFINE(z_libc_partition);
#endif /* Z_LIBC_PARTITION_EXISTS */

#ifdef CONFIG_MBEDTLS
K_APPMEM_PARTITION_DEFINE(k_mbedtls_partition);
#endif /* CONFIG_MBEDTLS */

#include <zephyr/logging/log.h>
LOG_MODULE_DECLARE(os, CONFIG_KERNEL_LOG_LEVEL);

#ifdef CONFIG_DYNAMIC_OBJECTS
static struct k_spinlock lists_lock;       /* kobj dlist */
static struct k_spinlock objfree_lock;     /* k_object_free */

#ifdef CONFIG_GEN_PRIV_STACKS
#if defined(CONFIG_ARM_MPU) || defined(CONFIG_ARC_MPU) || defined(CONFIG_RISCV_PMP)
#define STACK_ELEMENT_DATA_SIZE(size) \
	(sizeof(struct z_stack_data) + CONFIG_PRIVILEGED_STACK_SIZE + \
	Z_THREAD_STACK_OBJ_ALIGN(size) + K_THREAD_STACK_LEN(size))
#else
#define STACK_ELEMENT_DATA_SIZE(size) (sizeof(struct z_stack_data) + \
	K_THREAD_STACK_LEN(size))
#endif /* CONFIG_ARM_MPU || CONFIG_ARC_MPU || CONFIG_RISCV_PMP */
#else
#define STACK_ELEMENT_DATA_SIZE(size) K_THREAD_STACK_LEN(size)
#endif /* CONFIG_GEN_PRIV_STACKS */

#endif /* CONFIG_DYNAMIC_OBJECTS */
static struct k_spinlock obj_lock;         /* kobj struct data */

#define MAX_THREAD_BITS (CONFIG_MAX_THREAD_BYTES * BITS_PER_BYTE)

#ifdef CONFIG_DYNAMIC_OBJECTS
extern uint8_t _thread_idx_map[CONFIG_MAX_THREAD_BYTES];
#endif /* CONFIG_DYNAMIC_OBJECTS */

static void clear_perms_cb(struct k_object *ko, void *ctx_ptr);

const char *otype_to_str(enum k_objects otype)
{
	const char *ret;
#ifdef CONFIG_LOG
	switch (otype) {
	case K_OBJ_ANY:
		ret = "generic";
		break;
#include <zephyr/otype-to-str.h>
	default:
		ret = "?";
		break;
	}
#else
	ARG_UNUSED(otype);
	ret = NULL;
#endif /* CONFIG_LOG */
	return ret;
}

struct perm_ctx {
	int parent_id;
	int child_id;
	struct k_thread *parent;
};

#ifdef CONFIG_GEN_PRIV_STACKS
uint8_t *z_priv_stack_find(k_thread_stack_t *stack)
{
	struct k_object *obj = k_object_find(stack);

	__ASSERT(obj != NULL, "stack object not found");
	__ASSERT(obj->type == K_OBJ_THREAD_STACK_ELEMENT,
		 "bad stack object");

	return obj->data.stack_data->priv;
}
#endif /* CONFIG_GEN_PRIV_STACKS */

#ifdef CONFIG_DYNAMIC_OBJECTS

#ifdef ARCH_DYNAMIC_OBJ_K_THREAD_ALIGNMENT
#define DYN_OBJ_DATA_ALIGN_K_THREAD	(ARCH_DYNAMIC_OBJ_K_THREAD_ALIGNMENT)
#else
#define DYN_OBJ_DATA_ALIGN_K_THREAD	(sizeof(void *))
#endif /* ARCH_DYNAMIC_OBJ_K_THREAD_ALIGNMENT */

#ifdef CONFIG_DYNAMIC_THREAD_STACK_SIZE
#if defined(CONFIG_MPU_STACK_GUARD) || defined(CONFIG_PMP_STACK_GUARD)
#define DYN_OBJ_DATA_ALIGN_K_THREAD_STACK \
	Z_THREAD_STACK_OBJ_ALIGN(CONFIG_DYNAMIC_THREAD_STACK_SIZE)
#else
#define DYN_OBJ_DATA_ALIGN_K_THREAD_STACK \
	Z_THREAD_STACK_OBJ_ALIGN(CONFIG_PRIVILEGED_STACK_SIZE)
#endif /* CONFIG_MPU_STACK_GUARD || CONFIG_PMP_STACK_GUARD */
#else
#define DYN_OBJ_DATA_ALIGN_K_THREAD_STACK \
	Z_THREAD_STACK_OBJ_ALIGN(ARCH_STACK_PTR_ALIGN)
#endif /* CONFIG_DYNAMIC_THREAD_STACK_SIZE */

#define DYN_OBJ_DATA_ALIGN		\
	MAX(DYN_OBJ_DATA_ALIGN_K_THREAD, (sizeof(void *)))

struct dyn_obj {
	struct k_object kobj;
	sys_dnode_t dobj_list;

	/* The object itself */
	void *data;
};

extern struct k_object *z_object_gperf_find(const void *obj);
extern void z_object_gperf_wordlist_foreach(_wordlist_cb_func_t func,
					     void *context);

static sys_dlist_t obj_list = SYS_DLIST_STATIC_INIT(&obj_list);

static size_t obj_size_get(enum k_objects otype)
{
	size_t ret;

	switch (otype) {
#include <zephyr/otype-to-size.h>
	default:
		ret = sizeof(const struct device);
		break;
	}

	return ret;
}

static size_t obj_align_get(enum k_objects otype)
{
	size_t ret;

	switch (otype) {
	case K_OBJ_THREAD:
#ifdef ARCH_DYNAMIC_OBJ_K_THREAD_ALIGNMENT
		ret = ARCH_DYNAMIC_OBJ_K_THREAD_ALIGNMENT;
#else
		ret = __alignof(struct dyn_obj);
#endif /* ARCH_DYNAMIC_OBJ_K_THREAD_ALIGNMENT */
		break;
	default:
		ret = __alignof(struct dyn_obj);
		break;
	}

	return ret;
}

static struct dyn_obj *dyn_object_find(const void *obj)
{
	struct dyn_obj *node;
	k_spinlock_key_t key;

	key = k_spin_lock(&lists_lock);

	SYS_DLIST_FOR_EACH_CONTAINER(&obj_list, node, dobj_list) {
		if (node->kobj.name == obj) {
			goto end;
		}
	}

	/* No object found */
	node = NULL;

 end:
	k_spin_unlock(&lists_lock, key);

	return node;
}

static unsigned int thread_index_get(struct k_thread *thread)
{
	struct k_object *ko;

	ko = k_object_find(thread);

	if (ko == NULL) {
		return -1;
	}

	return ko->data.thread_id;
}

/*
 * `sched_locked` routes this path through the _sched_locked variants of
 * k_free / k_msgq_cleanup / k_stack_cleanup, avoiding recursive
 * acquisition of _sched_spinlock when we are reached from the abort
 * path (k_thread_perms_all_clear -> clear_perms_sched_locked_cb ->
 * unref_check). Matches upstream Zephyr fix 9cef0da05c3.
 */
static void unref_check(struct k_object *ko, uintptr_t index, bool sched_locked)
{
	k_spinlock_key_t key = k_spin_lock(&obj_lock);

	sys_bitfield_clear_bit((mem_addr_t)&ko->perms, index);

#ifdef CONFIG_DYNAMIC_OBJECTS
	if ((ko->flags & K_OBJ_FLAG_ALLOC) == 0U) {
		/* skip unref check for static kernel object */
		goto out;
	}

	void *vko = ko;

	struct dyn_obj *dyn = CONTAINER_OF(vko, struct dyn_obj, kobj);

	__ASSERT(IS_PTR_ALIGNED(dyn, struct dyn_obj), "unaligned z_object");

	for (int i = 0; i < CONFIG_MAX_THREAD_BYTES; i++) {
		if (ko->perms[i] != 0U) {
			goto out;
		}
	}

	switch (ko->type) {
	case K_OBJ_MSGQ:
		if (sched_locked) {
			z_msgq_cleanup_sched_locked((struct k_msgq *)ko->name);
		} else {
			k_msgq_cleanup((struct k_msgq *)ko->name);
		}
		break;
	case K_OBJ_STACK:
		if (sched_locked) {
			z_stack_cleanup_sched_locked((struct k_stack *)ko->name);
		} else {
			k_stack_cleanup((struct k_stack *)ko->name);
		}
		break;
	default:
		/* Nothing to do */
		break;
	}

	sys_dlist_remove(&dyn->dobj_list);
	if (sched_locked) {
		k_free_sched_locked(dyn->data);
		k_free_sched_locked(dyn);
	} else {
		k_free(dyn->data);
		k_free(dyn);
	}
out:
#endif /* CONFIG_DYNAMIC_OBJECTS */
	k_spin_unlock(&obj_lock, key);
}

static void wordlist_cb(struct k_object *ko, void *ctx_ptr)
{
	struct perm_ctx *ctx = (struct perm_ctx *)ctx_ptr;

	if (sys_bitfield_test_bit((mem_addr_t)&ko->perms, ctx->parent_id) &&
				  ((struct k_thread *)ko->name != ctx->parent)) {
		sys_bitfield_set_bit((mem_addr_t)&ko->perms, ctx->child_id);
	}
}

static bool thread_idx_alloc(uintptr_t *tidx)
{
	int i;
	int idx;
	int base;

	base = 0;
	for (i = 0; i < CONFIG_MAX_THREAD_BYTES; i++) {
		idx = find_lsb_set(_thread_idx_map[i]);

		if (idx != 0) {
			*tidx = base + (idx - 1);

			_thread_idx_map[i] &= ~(BIT(idx - 1));

			k_object_wordlist_foreach(clear_perms_cb,
						   (void *)*tidx);

			return true;
		}

		base += 8;
	}

	return false;
}

static void thread_idx_free(uintptr_t tidx)
{
	/* To prevent leaked permission when index is recycled */
	k_object_wordlist_foreach(clear_perms_cb, (void *)tidx);

	int base = tidx / NUM_BITS(_thread_idx_map[0]);
	int offset = tidx % NUM_BITS(_thread_idx_map[0]);

	_thread_idx_map[base] |= BIT(offset);
}

static struct k_object *dynamic_object_create(enum k_objects otype, size_t align,
					      size_t size)
{
	struct dyn_obj *dyn;

	dyn = z_thread_aligned_alloc(align, sizeof(struct dyn_obj));
	if (dyn == NULL) {
		return NULL;
	}

	if (otype == K_OBJ_THREAD_STACK_ELEMENT) {
		size_t adjusted_size;

		if (size == 0) {
			k_free(dyn);
			return NULL;
		}

		adjusted_size = STACK_ELEMENT_DATA_SIZE(size);
		dyn->data = z_thread_aligned_alloc(DYN_OBJ_DATA_ALIGN_K_THREAD_STACK,
						     adjusted_size);
		if (dyn->data == NULL) {
			k_free(dyn);
			return NULL;
		}

#ifdef CONFIG_GEN_PRIV_STACKS
		struct z_stack_data *stack_data = (struct z_stack_data *)
			((uint8_t *)dyn->data + adjusted_size - sizeof(*stack_data));
#if defined(CONFIG_ARM_MPU) || defined(CONFIG_ARC_MPU) || defined(CONFIG_RISCV_PMP)
		stack_data->priv = (void *)ROUND_UP(((uint8_t *)dyn->data + size),
			  Z_THREAD_STACK_OBJ_ALIGN(size));
#else
		stack_data->priv = (uint8_t *)dyn->data;
#endif /* CONFIG_ARM_MPU || CONFIG_ARC_MPU || CONFIG_RISCV_PMP */
		stack_data->size = adjusted_size;
		dyn->kobj.data.stack_data = stack_data;
		dyn->kobj.name = dyn->data;
#else
		dyn->kobj.name = dyn->data;
		dyn->kobj.data.stack_size = adjusted_size;
#endif /* CONFIG_GEN_PRIV_STACKS */
	} else {
		dyn->data = z_thread_aligned_alloc(align, obj_size_get(otype) + size);
		if (dyn->data == NULL) {
			k_free(dyn);
			return NULL;
		}
		dyn->kobj.name = dyn->data;
	}

	dyn->kobj.type = otype;
	dyn->kobj.flags = 0;
	(void)memset(dyn->kobj.perms, 0, CONFIG_MAX_THREAD_BYTES);

	k_spinlock_key_t key = k_spin_lock(&lists_lock);

	sys_dlist_append(&obj_list, &dyn->dobj_list);
	k_spin_unlock(&lists_lock, key);

	return &dyn->kobj;
}

struct k_object *k_object_create_dynamic_aligned(size_t align, size_t size)
{
	struct k_object *obj = dynamic_object_create(K_OBJ_ANY, align, size);

	if (obj == NULL) {
		LOG_ERR("could not allocate kernel object, out of memory");
	}

	return obj;
}

static void *z_object_alloc(enum k_objects otype, size_t size)
{
	struct k_object *zo;
	uintptr_t tidx = 0;

	if ((otype <= K_OBJ_ANY) || (otype >= K_OBJ_LAST)) {
		LOG_ERR("bad object type %d requested", otype);
		return NULL;
	}

	switch (otype) {
	case K_OBJ_THREAD:
		if (!thread_idx_alloc(&tidx)) {
			LOG_ERR("out of free thread indexes");
			return NULL;
		}
		break;
	/* The following are currently not allowed at all */
	case K_OBJ_FUTEX:			/* Lives in user memory */
	case K_OBJ_SYS_MUTEX:			/* Lives in user memory */
	case K_OBJ_NET_SOCKET:			/* Indeterminate size */
		LOG_ERR("forbidden object type '%s' requested",
			otype_to_str(otype));
		return NULL;
	default:
		/* Remainder within bounds are permitted */
		break;
	}

	zo = dynamic_object_create(otype, obj_align_get(otype), size);
	if (zo == NULL) {
		if (otype == K_OBJ_THREAD) {
			thread_idx_free(tidx);
		}
		return NULL;
	}

	if (otype == K_OBJ_THREAD) {
		zo->data.thread_id = tidx;
	}

	k_thread_perms_set(zo, _current);

	zo->flags |= K_OBJ_FLAG_ALLOC;

	return zo->name;
}

void *z_impl_k_object_alloc(enum k_objects otype)
{
	return z_object_alloc(otype, 0);
}

void *z_impl_k_object_alloc_size(enum k_objects otype, size_t size)
{
	return z_object_alloc(otype, size);
}

void k_object_free(void *obj)
{
	struct dyn_obj *dyn;

	k_spinlock_key_t key = k_spin_lock(&objfree_lock);

	dyn = dyn_object_find(obj);
	if (dyn != NULL) {
		sys_dlist_remove(&dyn->dobj_list);

		if (dyn->kobj.type == K_OBJ_THREAD) {
			thread_idx_free(dyn->kobj.data.thread_id);
		}
	}
	k_spin_unlock(&objfree_lock, key);

	if (dyn != NULL) {
		k_free(dyn->data);
		k_free(dyn);
	}
}

struct k_object *k_object_find(const void *obj)
{
	struct k_object *ret;

	ret = z_object_gperf_find(obj);

	if (ret == NULL) {
		struct dyn_obj *dyn;

		dyn = dyn_object_find(obj);
		if (dyn != NULL) {
			ret = &dyn->kobj;
		}
	}

	return ret;
}

void k_object_wordlist_foreach(_wordlist_cb_func_t func, void *context)
{
	struct dyn_obj *obj, *next;

	z_object_gperf_wordlist_foreach(func, context);

	k_spinlock_key_t key = k_spin_lock(&lists_lock);

	SYS_DLIST_FOR_EACH_CONTAINER_SAFE(&obj_list, obj, next, dobj_list) {
		func(&obj->kobj, context);
	}
	k_spin_unlock(&lists_lock, key);
}
#endif /* CONFIG_DYNAMIC_OBJECTS */

#ifdef CONFIG_DYNAMIC_OBJECTS
Z_GENERIC_SECTION(.kobject_data.text.dummies)
__weak struct k_object *z_object_gperf_find(const void *obj)
{
	return NULL;
}
Z_GENERIC_SECTION(.kobject_data.text.dummies)
__weak void z_object_gperf_wordlist_foreach(_wordlist_cb_func_t func, void *context)
{
}
#else
Z_GENERIC_SECTION(.kobject_data.text.dummies)
__weak struct k_object *k_object_find(const void *obj)
{
	return NULL;
}
Z_GENERIC_SECTION(.kobject_data.text.dummies)
__weak void k_object_wordlist_foreach(_wordlist_cb_func_t func, void *context)
{
}
#endif

#ifndef CONFIG_DYNAMIC_OBJECTS
static unsigned int thread_index_get(struct k_thread *thread)
{
	struct k_object *ko;

	ko = k_object_find(thread);

	if (ko == NULL) {
		return -1;
	}

	return ko->data.thread_id;
}
#endif /* !CONFIG_DYNAMIC_OBJECTS */

void k_thread_perms_inherit(struct k_thread *parent, struct k_thread *child)
{
	struct perm_ctx ctx = {
		thread_index_get(parent),
		thread_index_get(child),
		parent
	};

	if ((ctx.parent_id != -1) && (ctx.child_id != -1)) {
		k_object_wordlist_foreach(wordlist_cb, &ctx);
	}
}

void k_thread_perms_set(struct k_object *ko, struct k_thread *thread)
{
	int index = thread_index_get(thread);

	if (index != -1) {
		sys_bitfield_set_bit((mem_addr_t)&ko->perms, index);
	}
}

void k_thread_perms_clear(struct k_object *ko, struct k_thread *thread)
{
	int index = thread_index_get(thread);

	if (index != -1) {
		sys_bitfield_clear_bit((mem_addr_t)&ko->perms, index);
		unref_check(ko, index, false);
	}
}

static void clear_perms_cb(struct k_object *ko, void *ctx_ptr)
{
	uintptr_t id = (uintptr_t)ctx_ptr;

	unref_check(ko, id, false);
}

/*
 * Abort-path callback — reached from k_thread_perms_all_clear while
 * _sched_spinlock is held. Routes unref_check's free/cleanup through
 * the _sched_locked variants.
 */
static void clear_perms_sched_locked_cb(struct k_object *ko, void *ctx_ptr)
{
	uintptr_t id = (uintptr_t)ctx_ptr;

	unref_check(ko, id, true);
}

void k_thread_perms_all_clear(struct k_thread *thread)
{
	uintptr_t index = thread_index_get(thread);

	if ((int)index != -1) {
		k_object_wordlist_foreach(clear_perms_sched_locked_cb,
					 (void *)index);
	}
}

/*
 * Gale: thread_perms_test — Extract->Decide->Apply
 *
 * Extract: read ko->flags and per-thread permission bit.
 * Decide:  Rust determines whether access is granted (US1, US5).
 * Apply:   return the decision.
 */
static int thread_perms_test(struct k_object *ko)
{
	/* Extract: read the PUBLIC flag */
	uint8_t flags = ko->flags;

	/* Extract: read the per-thread permission bit */
	int index = thread_index_get(_current);
	uint8_t has_perm_bit = 0U;

	if (index != -1) {
		has_perm_bit = sys_bitfield_test_bit(
			(mem_addr_t)&ko->perms, index) ? 1U : 0U;
	}

	/* Decide: Rust determines access */
	struct gale_userspace_access_decision d =
		gale_k_object_access_decide(flags, has_perm_bit);

	/* Apply: return Rust's decision */
	return d.granted;
}

static void dump_permission_error(struct k_object *ko)
{
	int index = thread_index_get(_current);
	LOG_ERR("thread %p (%d) does not have permission on %s %p",
		_current, index,
		otype_to_str(ko->type), ko->name);
	LOG_HEXDUMP_ERR(ko->perms, sizeof(ko->perms), "permission bitmap");
}

void k_object_dump_error(int retval, const void *obj, struct k_object *ko,
			enum k_objects otype)
{
	switch (retval) {
	case -EBADF:
		LOG_ERR("%p is not a valid %s", obj, otype_to_str(otype));
		if (ko == NULL) {
			LOG_ERR("address is not a known kernel object");
		} else {
			LOG_ERR("address is actually a %s",
				otype_to_str(ko->type));
		}
		break;
	case -EPERM:
		dump_permission_error(ko);
		break;
	case -EINVAL:
		LOG_ERR("%p used before initialization", obj);
		break;
	case -EADDRINUSE:
		LOG_ERR("%p %s in use", obj, otype_to_str(otype));
		break;
	default:
		/* Not handled error */
		break;
	}
}

/*
 * Gale: k_object_access_check — thin wrapper around k_object_validate.
 *
 * Added upstream (>=v4.4) as a first-class syscall; our shim defers to
 * k_object_validate (which already carries the full US1/US4/US5/US7
 * verification path).
 */
int z_impl_k_object_access_check(const void *object)
{
	return k_object_validate(k_object_find(object), K_OBJ_ANY, _OBJ_INIT_ANY);
}

void z_impl_k_object_access_grant(const void *object, struct k_thread *thread)
{
	struct k_object *ko = k_object_find(object);

	if (ko != NULL) {
		k_thread_perms_set(ko, thread);
	}
}

void k_object_access_revoke(const void *object, struct k_thread *thread)
{
	struct k_object *ko = k_object_find(object);

	if (ko != NULL) {
		k_thread_perms_clear(ko, thread);
	}
}

void z_impl_k_object_release(const void *object)
{
	k_object_access_revoke(object, _current);
}

/*
 * Gale: k_object_access_all_grant — Extract->Decide->Apply
 *
 * Verified: US5 (public flag grants universal access).
 */
void k_object_access_all_grant(const void *object)
{
	struct k_object *ko = k_object_find(object);

	if (ko != NULL) {
		/* Decide: Rust computes new flags with PUBLIC set */
		struct gale_userspace_public_decision d =
			gale_k_object_make_public_decide(ko->flags);

		/* Apply: write new flags */
		ko->flags = d.new_flags;
	}
}

/*
 * Gale: k_object_validate — Extract->Decide->Apply
 *
 * Extract: read ko->type, ko->flags, and access check result.
 * Decide:  Rust determines pass/fail (US1, US4, US5, US7).
 * Apply:   return the error code.
 */
int k_object_validate(struct k_object *ko, enum k_objects otype,
		       enum _obj_init_check init)
{
	/* Extract: null check — C-only, not modeled in Rust */
	if (unlikely(ko == NULL)) {
		return -EBADF;
	}

	/* Extract: read state from kernel object */
	uint8_t obj_type = (uint8_t)ko->type;
	uint8_t expected_type = (uint8_t)otype;
	uint8_t flags = ko->flags;
	uint8_t has_access = (thread_perms_test(ko) != 0) ? 1U : 0U;
	int8_t init_check = (int8_t)init;

	/* Decide: Rust determines validation outcome */
	struct gale_userspace_validate_decision d =
		gale_k_object_validate_decide(
			obj_type, expected_type, flags,
			has_access, init_check);

	/* Apply: return Rust's verdict */
	return d.ret;
}

/*
 * Gale: k_object_init — Extract->Decide->Apply
 *
 * Verified: US7 (initialization flag management).
 */
void k_object_init(const void *obj)
{
	struct k_object *ko;

	ko = k_object_find(obj);
	if (ko == NULL) {
		return;
	}

	/* Decide: Rust computes new flags with INITIALIZED set */
	struct gale_userspace_init_decision d =
		gale_k_object_init_decide(ko->flags);

	/* Apply: write new flags */
	ko->flags = d.new_flags;
}

/*
 * Gale: k_object_recycle — Extract->Decide->Apply
 *
 * Verified: US2 (grant), US6 (clear perms), US7 (init).
 */
void k_object_recycle(const void *obj)
{
	struct k_object *ko = k_object_find(obj);

	if (ko != NULL) {
		/* Decide: Rust determines new flags and perm clear */
		struct gale_userspace_recycle_decision d =
			gale_k_object_recycle_decide(ko->flags);

		/* Apply: execute Rust's decision */
		if (d.clear_perms) {
			(void)memset(ko->perms, 0, sizeof(ko->perms));
			k_thread_perms_set(ko, _current);
		}
		ko->flags = d.new_flags;
	}
}

/*
 * Gale: k_object_uninit — Extract->Decide->Apply
 *
 * Verified: US7 (initialization flag management).
 */
void k_object_uninit(const void *obj)
{
	struct k_object *ko;

	ko = k_object_find(obj);
	if (ko == NULL) {
		return;
	}

	/* Decide: Rust computes new flags with INITIALIZED cleared */
	struct gale_userspace_uninit_decision d =
		gale_k_object_uninit_decide(ko->flags);

	/* Apply: write new flags */
	ko->flags = d.new_flags;
}

/*
 * Copy to/from helper functions used in syscall handlers
 */
void *k_usermode_alloc_from_copy(const void *src, size_t size)
{
	void *dst = NULL;

	/* Does the caller in user mode have access to read this memory? */
	if (K_SYSCALL_MEMORY_READ(src, size)) {
		goto out_err;
	}

	dst = z_thread_malloc(size);
	if (dst == NULL) {
		LOG_ERR("out of thread resource pool memory (%zu)", size);
		goto out_err;
	}

	(void)memcpy(dst, src, size);
out_err:
	return dst;
}

static int user_copy(void *dst, const void *src, size_t size, bool to_user)
{
	int ret = EFAULT;

	/* Does the caller in user mode have access to this memory? */
	if (to_user ? K_SYSCALL_MEMORY_WRITE(dst, size) :
			K_SYSCALL_MEMORY_READ(src, size)) {
		goto out_err;
	}

	(void)memcpy(dst, src, size);
	ret = 0;
out_err:
	return ret;
}

int k_usermode_from_copy(void *dst, const void *src, size_t size)
{
	return user_copy(dst, src, size, false);
}

int k_usermode_to_copy(void *dst, const void *src, size_t size)
{
	return user_copy(dst, src, size, true);
}

char *k_usermode_string_alloc_copy(const char *src, size_t maxlen)
{
	size_t actual_len;
	int err;
	char *ret = NULL;

	actual_len = k_usermode_string_nlen(src, maxlen, &err);
	if (err != 0) {
		goto out;
	}
	if (actual_len == maxlen) {
		/* Not NULL terminated */
		LOG_ERR("string too long %p (%zu)", src, actual_len);
		goto out;
	}
	if (size_add_overflow(actual_len, 1, &actual_len)) {
		LOG_ERR("overflow");
		goto out;
	}

	ret = k_usermode_alloc_from_copy(src, actual_len);

	if (ret != NULL) {
		ret[actual_len - 1U] = '\0';
	}
out:
	return ret;
}

int k_usermode_string_copy(char *dst, const char *src, size_t maxlen)
{
	size_t actual_len;
	int ret, err;

	actual_len = k_usermode_string_nlen(src, maxlen, &err);
	if (err != 0) {
		ret = EFAULT;
		goto out;
	}
	if (actual_len == maxlen) {
		/* Not NULL terminated */
		LOG_ERR("string too long %p (%zu)", src, actual_len);
		ret = EINVAL;
		goto out;
	}
	if (size_add_overflow(actual_len, 1, &actual_len)) {
		LOG_ERR("overflow");
		ret = EINVAL;
		goto out;
	}

	ret = k_usermode_from_copy(dst, src, actual_len);

	/* See comment above in k_usermode_string_alloc_copy() */
	dst[actual_len - 1] = '\0';
out:
	return ret;
}

/*
 * Application memory region initialization
 */

extern char __app_shmem_regions_start[];
extern char __app_shmem_regions_end[];

static int app_shmem_bss_zero(void)
{
	struct z_app_region *region, *end;


	end = (struct z_app_region *)&__app_shmem_regions_end[0];
	region = (struct z_app_region *)&__app_shmem_regions_start[0];

	for ( ; region < end; region++) {
#if defined(CONFIG_DEMAND_PAGING) && !defined(CONFIG_LINKER_GENERIC_SECTIONS_PRESENT_AT_BOOT)
		extern bool z_sys_post_kernel;
		bool do_clear = z_sys_post_kernel;

		if (((uint8_t *)region->bss_start >= (uint8_t *)_app_smem_pinned_start) &&
		    ((uint8_t *)region->bss_start < (uint8_t *)_app_smem_pinned_end)) {
			do_clear = !do_clear;
		}

		if (do_clear)
#endif /* CONFIG_DEMAND_PAGING && !CONFIG_LINKER_GENERIC_SECTIONS_PRESENT_AT_BOOT */
		{
			(void)memset(region->bss_start, 0, region->bss_size);
		}
	}

	return 0;
}

SYS_INIT_NAMED(app_shmem_bss_zero_pre, app_shmem_bss_zero,
	       PRE_KERNEL_1, CONFIG_KERNEL_INIT_PRIORITY_DEFAULT);

#if defined(CONFIG_DEMAND_PAGING) && !defined(CONFIG_LINKER_GENERIC_SECTIONS_PRESENT_AT_BOOT)
SYS_INIT_NAMED(app_shmem_bss_zero_post, app_shmem_bss_zero,
	       POST_KERNEL, CONFIG_KERNEL_INIT_PRIORITY_DEFAULT);
#endif /* CONFIG_DEMAND_PAGING && !CONFIG_LINKER_GENERIC_SECTIONS_PRESENT_AT_BOOT */

/*
 * Default handlers if otherwise unimplemented
 */

static uintptr_t handler_bad_syscall(uintptr_t bad_id, uintptr_t arg2,
				     uintptr_t arg3, uintptr_t arg4,
				     uintptr_t arg5, uintptr_t arg6,
				     void *ssf)
{
	ARG_UNUSED(arg2);
	ARG_UNUSED(arg3);
	ARG_UNUSED(arg4);
	ARG_UNUSED(arg5);
	ARG_UNUSED(arg6);

	LOG_ERR("Bad system call id %" PRIuPTR " invoked", bad_id);
	arch_syscall_oops(ssf);
	CODE_UNREACHABLE; /* LCOV_EXCL_LINE */
}

static uintptr_t handler_no_syscall(uintptr_t arg1, uintptr_t arg2,
				    uintptr_t arg3, uintptr_t arg4,
				    uintptr_t arg5, uintptr_t arg6, void *ssf)
{
	ARG_UNUSED(arg1);
	ARG_UNUSED(arg2);
	ARG_UNUSED(arg3);
	ARG_UNUSED(arg4);
	ARG_UNUSED(arg5);
	ARG_UNUSED(arg6);

	LOG_ERR("Unimplemented system call");
	arch_syscall_oops(ssf);
	CODE_UNREACHABLE; /* LCOV_EXCL_LINE */
}

#include <zephyr/syscall_dispatch.c>
