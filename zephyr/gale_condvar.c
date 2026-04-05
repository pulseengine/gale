/*
 * Copyright (c) 2016 Wind River Systems, Inc.
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale condvar — phase 2: Extract→Decide→Apply pattern.
 *
 * This replaces kernel/condvar.c.  C extracts kernel state (wait queue
 * length, timeout mode), Rust decides the action (no-op, wake-one,
 * wake-all, or pend), C applies side effects (z_unpend_first_thread,
 * z_ready_thread, z_pend_curr, mutex lock/unlock, rescheduling).
 *
 * Verified operations (Verus proofs):
 *   gale_k_condvar_signal_decide    — C2 (at most one waiter woken)
 *                                     C3 (no-op when queue empty)
 *                                     C7 (priority ordering preserved)
 *   gale_k_condvar_broadcast_decide — C4 (all waiters woken)
 *                                     C5 (returns 0 when empty)
 *                                     C8 (no overflow in woken count)
 *   gale_k_condvar_wait_decide      — C6 (thread added to wait queue)
 */

#include <zephyr/kernel.h>
#include <zephyr/kernel_structs.h>
#include <zephyr/toolchain.h>
#include <wait_q.h>
#include <zephyr/sys/dlist.h>
#include <ksched.h>
#include <zephyr/init.h>
#include <zephyr/internal/syscall_handler.h>
#include <zephyr/tracing/tracing.h>
#include <zephyr/sys/check.h>

#include "gale_condvar.h"

static struct k_spinlock condvar_lock;

#ifdef CONFIG_OBJ_CORE_CONDVAR
static struct k_obj_type obj_type_condvar;
#endif /* CONFIG_OBJ_CORE_CONDVAR */

int z_impl_k_condvar_init(struct k_condvar *condvar)
{
	z_waitq_init(&condvar->wait_q);
	k_object_init(condvar);

#ifdef CONFIG_OBJ_CORE_CONDVAR
	k_obj_core_init_and_link(K_OBJ_CORE(condvar), &obj_type_condvar);
#endif

	SYS_PORT_TRACING_OBJ_INIT(k_condvar, condvar, 0);

	return 0;
}

#ifdef CONFIG_USERSPACE
static inline int z_vrfy_k_condvar_init(struct k_condvar *condvar)
{
	K_OOPS(K_SYSCALL_OBJ_INIT(condvar, K_OBJ_CONDVAR));
	return z_impl_k_condvar_init(condvar);
}
#include <zephyr/syscalls/k_condvar_init_mrsh.c>
#endif /* CONFIG_USERSPACE */

int z_impl_k_condvar_signal(struct k_condvar *condvar)
{
	k_spinlock_key_t key = k_spin_lock(&condvar_lock);

	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_condvar, signal, condvar);

	/* Extract: is the wait queue non-empty? */
	bool has_waiter = !z_waitq_is_empty(&condvar->wait_q);

	/* Decide: Rust determines NOOP or WAKE_ONE */
	struct gale_condvar_signal_decision d =
		gale_k_condvar_signal_decide(has_waiter ? 1U : 0U);

	/* Apply */
	if (d.action == GALE_CONDVAR_SIGNAL_WAKE_ONE) {
		struct k_thread *thread = z_unpend_first_thread(&condvar->wait_q);

		if (thread != NULL) {
			arch_thread_return_value_set(thread, 0);
			z_ready_thread(thread);
			z_reschedule(&condvar_lock, key);
			goto out_traced;
		}
	}

	k_spin_unlock(&condvar_lock, key);

out_traced:
	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_condvar, signal, condvar, 0);

	return 0;
}

#ifdef CONFIG_USERSPACE
static inline int z_vrfy_k_condvar_signal(struct k_condvar *condvar)
{
	K_OOPS(K_SYSCALL_OBJ(condvar, K_OBJ_CONDVAR));
	return z_impl_k_condvar_signal(condvar);
}
#include <zephyr/syscalls/k_condvar_signal_mrsh.c>
#endif /* CONFIG_USERSPACE */

int z_impl_k_condvar_broadcast(struct k_condvar *condvar)
{
	struct k_thread *pending;
	k_spinlock_key_t key = k_spin_lock(&condvar_lock);
	int woken = 0;
	bool resched = false;

	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_condvar, broadcast, condvar);

	/* Extract: current wait queue length */
	uint32_t num_waiters = 0;
	{
		struct k_thread *t = z_waitq_head(&condvar->wait_q);

		while (t != NULL) {
			num_waiters++;
			t = (struct k_thread *)t->base.pended_on;
			/* guard against corrupt list */
			if (num_waiters > CONFIG_MAX_THREAD_BYTES * 8U) {
				break;
			}
		}
	}

	/* Decide: Rust validates the woken count (C8 no overflow) */
	struct gale_condvar_broadcast_decision d =
		gale_k_condvar_broadcast_decide(num_waiters);

	/* Apply: wake d.woken threads */
	for (uint32_t i = 0; i < d.woken; i++) {
		pending = z_unpend_first_thread(&condvar->wait_q);
		if (pending == NULL) {
			break;
		}
		woken++;
		arch_thread_return_value_set(pending, 0);
		z_ready_thread(pending);
		resched = true;
	}

	if (resched) {
		z_reschedule(&condvar_lock, key);
	} else {
		k_spin_unlock(&condvar_lock, key);
	}

	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_condvar, broadcast, condvar, woken);

	return woken;
}

#ifdef CONFIG_USERSPACE
static inline int z_vrfy_k_condvar_broadcast(struct k_condvar *condvar)
{
	K_OOPS(K_SYSCALL_OBJ(condvar, K_OBJ_CONDVAR));
	return z_impl_k_condvar_broadcast(condvar);
}
#include <zephyr/syscalls/k_condvar_broadcast_mrsh.c>
#endif /* CONFIG_USERSPACE */

int z_impl_k_condvar_wait(struct k_condvar *condvar, struct k_mutex *mutex,
			  k_timeout_t timeout)
{
	int ret;
	k_spinlock_key_t key;

	__ASSERT(!arch_is_in_isr(), "condvar wait cannot be used in ISR");

	/* STPA GAP-8: runtime ISR guard */
	CHECKIF(arch_is_in_isr()) {
		return -ENOTSUP;
	}

	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_condvar, wait, condvar, mutex,
					 timeout);

	/* Decide: should we pend or return EAGAIN? */
	struct gale_condvar_wait_decision d =
		gale_k_condvar_wait_decide(
			K_TIMEOUT_EQ(timeout, K_NO_WAIT) ? 1U : 0U);

	if (d.action == GALE_CONDVAR_WAIT_RETURN_EAGAIN) {
		SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_condvar, wait, condvar, mutex,
						timeout, d.ret);
		return d.ret;
	}

	/* Apply: release mutex, pend on condvar, re-acquire mutex */
	key = k_spin_lock(&condvar_lock);

	k_mutex_unlock(mutex);

	ret = z_pend_curr(&condvar_lock, key, &condvar->wait_q, timeout);

	if (ret == 0) {
		k_mutex_lock(mutex, K_FOREVER);
	}

	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_condvar, wait, condvar, mutex,
					timeout, ret);

	return ret;
}

#ifdef CONFIG_USERSPACE
static inline int z_vrfy_k_condvar_wait(struct k_condvar *condvar,
					 struct k_mutex *mutex,
					 k_timeout_t timeout)
{
	K_OOPS(K_SYSCALL_OBJ(condvar, K_OBJ_CONDVAR));
	K_OOPS(K_SYSCALL_OBJ(mutex, K_OBJ_MUTEX));
	return z_impl_k_condvar_wait(condvar, mutex, timeout);
}
#include <zephyr/syscalls/k_condvar_wait_mrsh.c>
#endif /* CONFIG_USERSPACE */

#ifdef CONFIG_OBJ_CORE_CONDVAR
static int init_condvar_obj_core_list(void)
{
	z_obj_type_init(&obj_type_condvar, K_OBJ_TYPE_CONDVAR_ID,
			offsetof(struct k_condvar, obj_core));

	STRUCT_SECTION_FOREACH(k_condvar, condvar) {
		k_obj_core_init_and_link(K_OBJ_CORE(condvar), &obj_type_condvar);
	}

	return 0;
}

SYS_INIT(init_condvar_obj_core_list, PRE_KERNEL_1,
	 CONFIG_KERNEL_INIT_PRIORITY_OBJECTS);
#endif /* CONFIG_OBJ_CORE_CONDVAR */
