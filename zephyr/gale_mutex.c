/*
 * Copyright (c) 2016 Wind River Systems, Inc.
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale mutex — phase 2: decision struct pattern.
 *
 * Rust decides the action (acquire/pend/busy for lock,
 * released/unlocked/error for unlock), C applies it.
 * Priority inheritance logic stays in C.
 *
 * Verified operations (Verus proofs):
 *   gale_k_mutex_lock_decide   — M3 (acquire), M4 (reentrant),
 *                                 M5 (contended), M10 (no overflow)
 *   gale_k_mutex_unlock_decide — M6a (EINVAL), M6b (EPERM),
 *                                 M7 (reentrant), M10 (no underflow)
 */

#include <zephyr/kernel.h>
#include <zephyr/kernel_structs.h>

#include <zephyr/toolchain.h>
#include <wait_q.h>
#include <ksched.h>
#include <kthread.h>
#include <zephyr/init.h>
#include <zephyr/internal/syscall_handler.h>
#include <zephyr/tracing/tracing.h>
#include <zephyr/sys/check.h>
#include <zephyr/logging/log.h>

#include "gale_mutex.h"

LOG_MODULE_DECLARE(os, CONFIG_KERNEL_LOG_LEVEL);

static struct k_spinlock lock;

#ifdef CONFIG_OBJ_CORE_MUTEX
static struct k_obj_type obj_type_mutex;
#endif /* CONFIG_OBJ_CORE_MUTEX */

int z_impl_k_mutex_init(struct k_mutex *mutex)
{
	mutex->owner = NULL;
	mutex->lock_count = 0U;

	z_waitq_init(&mutex->wait_q);

	k_object_init(mutex);

#ifdef CONFIG_OBJ_CORE_MUTEX
	k_obj_core_init_and_link(K_OBJ_CORE(mutex), &obj_type_mutex);
#endif /* CONFIG_OBJ_CORE_MUTEX */

	SYS_PORT_TRACING_OBJ_INIT(k_mutex, mutex, 0);

	return 0;
}

#ifdef CONFIG_USERSPACE
static inline int z_vrfy_k_mutex_init(struct k_mutex *mutex)
{
	K_OOPS(K_SYSCALL_OBJ_INIT(mutex, K_OBJ_MUTEX));
	return z_impl_k_mutex_init(mutex);
}
#include <zephyr/syscalls/k_mutex_init_mrsh.c>
#endif /* CONFIG_USERSPACE */

#if (CONFIG_PRIORITY_CEILING < K_LOWEST_THREAD_PRIO)
static int32_t new_prio_for_inheritance(int32_t target, int32_t limit)
{
	int new_prio = z_is_prio_higher(target, limit) ? target : limit;

	new_prio = z_get_new_prio_with_ceiling(new_prio);

	return new_prio;
}

static bool adjust_owner_prio(struct k_mutex *mutex, int32_t new_prio)
{
	if (mutex->owner->base.prio != new_prio) {

		LOG_DBG("%p (ready (y/n): %c) prio changed to %d (was %d)",
			mutex->owner, z_is_thread_ready(mutex->owner) ?
			'y' : 'n',
			new_prio, mutex->owner->base.prio);

		return z_thread_prio_set(mutex->owner, new_prio);
	}
	return false;
}
#endif

int z_impl_k_mutex_lock(struct k_mutex *mutex, k_timeout_t timeout)
{
	k_spinlock_key_t key;
#if (CONFIG_PRIORITY_CEILING < K_LOWEST_THREAD_PRIO)
	bool resched = false;
	int new_prio;
#endif

	__ASSERT(!arch_is_in_isr(), "mutexes cannot be used inside ISRs");

	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_mutex, lock, mutex, timeout);

	key = k_spin_lock(&lock);

	/* Decide: Rust determines action based on ownership and timeout */
	struct gale_mutex_lock_decision d = gale_k_mutex_lock_decide(
		mutex->lock_count,
		(mutex->owner == NULL) ? 1U : 0U,
		(mutex->owner == _current) ? 1U : 0U,
		K_TIMEOUT_EQ(timeout, K_NO_WAIT) ? 1U : 0U);

	/* Apply: execute Rust's decision */
	if (d.action == GALE_MUTEX_ACTION_ACQUIRED) {
		/* Lock acquired (new or reentrant). */

#if (CONFIG_PRIORITY_CEILING < K_LOWEST_THREAD_PRIO)
		mutex->owner_orig_prio = (mutex->lock_count == 0U) ?
					_current->base.prio :
					mutex->owner_orig_prio;
#endif

		mutex->lock_count = d.new_lock_count;
		mutex->owner = _current;

#if (CONFIG_PRIORITY_CEILING < K_LOWEST_THREAD_PRIO)
		LOG_DBG("%p took mutex %p, count: %d, orig prio: %d",
			_current, mutex, mutex->lock_count,
			mutex->owner_orig_prio);
#else
		LOG_DBG("%p took mutex %p, count: %d",
			_current, mutex, mutex->lock_count);
#endif

		k_spin_unlock(&lock, key);

		SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_mutex, lock, mutex,
						timeout, 0);

		return 0;
	}

	if (d.action == GALE_MUTEX_ACTION_BUSY) {
		/* No-wait or overflow: return immediately */
		k_spin_unlock(&lock, key);

		SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_mutex, lock, mutex,
						timeout, d.ret);

		return d.ret;
	}

	/* GALE_MUTEX_ACTION_PEND: block on wait queue */

	SYS_PORT_TRACING_OBJ_FUNC_BLOCKING(k_mutex, lock, mutex, timeout);

#if (CONFIG_PRIORITY_CEILING < K_LOWEST_THREAD_PRIO)
	new_prio = new_prio_for_inheritance(_current->base.prio,
					    mutex->owner->base.prio);

	LOG_DBG("adjusting prio up on mutex %p", mutex);

	if (z_is_prio_higher(new_prio, mutex->owner->base.prio)) {
		resched = adjust_owner_prio(mutex, new_prio);
	}
#endif

	int got_mutex = z_pend_curr(&lock, key, &mutex->wait_q, timeout);

	LOG_DBG("on mutex %p got_mutex value: %d", mutex, got_mutex);

	LOG_DBG("%p got mutex %p (y/n): %c", _current, mutex,
		got_mutex ? 'y' : 'n');

#if (CONFIG_PRIORITY_CEILING < K_LOWEST_THREAD_PRIO)
	if (got_mutex == 0) {
		SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_mutex, lock, mutex,
						timeout, 0);
		return 0;
	}

	/* timed out */

	LOG_DBG("%p timeout on mutex %p", _current, mutex);

	key = k_spin_lock(&lock);

	if (likely(mutex->owner != NULL)) {
		struct k_thread *waiter = z_waitq_head(&mutex->wait_q);

		new_prio = (waiter != NULL) ?
			new_prio_for_inheritance(waiter->base.prio,
						 mutex->owner_orig_prio) :
			mutex->owner_orig_prio;

		LOG_DBG("adjusting prio down on mutex %p", mutex);

		resched = adjust_owner_prio(mutex, new_prio) || resched;
	}

	if (resched) {
		z_reschedule(&lock, key);
	} else {
		k_spin_unlock(&lock, key);
	}

	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_mutex, lock, mutex,
					timeout, -EAGAIN);

	return -EAGAIN;
#else
	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_mutex, lock, mutex,
					timeout, got_mutex);

	return got_mutex;
#endif
}

#ifdef CONFIG_USERSPACE
static inline int z_vrfy_k_mutex_lock(struct k_mutex *mutex,
				      k_timeout_t timeout)
{
	K_OOPS(K_SYSCALL_OBJ(mutex, K_OBJ_MUTEX));
	return z_impl_k_mutex_lock(mutex, timeout);
}
#include <zephyr/syscalls/k_mutex_lock_mrsh.c>
#endif /* CONFIG_USERSPACE */

int z_impl_k_mutex_unlock(struct k_mutex *mutex)
{
	struct k_thread *new_owner;

	__ASSERT(!arch_is_in_isr(), "mutexes cannot be used inside ISRs");

	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_mutex, unlock, mutex);

	/* Decide: Rust determines action based on ownership */
	struct gale_mutex_unlock_decision d = gale_k_mutex_unlock_decide(
		mutex->lock_count,
		(mutex->owner == NULL) ? 1U : 0U,
		(mutex->owner == _current) ? 1U : 0U);

	/* Apply: execute Rust's decision */
	if (d.action == GALE_MUTEX_UNLOCK_ERROR) {
		/* -EINVAL or -EPERM */
		SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_mutex, unlock, mutex, d.ret);
		return d.ret;
	}

	LOG_DBG("mutex %p lock_count: %d", mutex, mutex->lock_count);

	if (d.action == GALE_MUTEX_UNLOCK_RELEASED) {
		/*
		 * Reentrant release: lock_count decremented, still held.
		 * Validated by Gale — no underflow.
		 */
		mutex->lock_count = d.new_lock_count;
		goto k_mutex_unlock_return;
	}

	/* GALE_MUTEX_UNLOCK_UNLOCKED: final unlock — handle waiters. */

	k_spinlock_key_t key = k_spin_lock(&lock);

#if (CONFIG_PRIORITY_CEILING < K_LOWEST_THREAD_PRIO)
	adjust_owner_prio(mutex, mutex->owner_orig_prio);
#endif

	/* Get the new owner, if any */
	new_owner = z_unpend_first_thread(&mutex->wait_q);

	mutex->owner = new_owner;

	LOG_DBG("new owner of mutex %p: %p (prio: %d)",
		mutex, new_owner, new_owner ? new_owner->base.prio : -1000);

	if (unlikely(new_owner != NULL)) {
#if (CONFIG_PRIORITY_CEILING < K_LOWEST_THREAD_PRIO)
		mutex->owner_orig_prio = new_owner->base.prio;
#endif
		mutex->lock_count = 1U;
		arch_thread_return_value_set(new_owner, 0);
		z_ready_thread(new_owner);
		z_reschedule(&lock, key);
	} else {
		mutex->lock_count = 0U;
		k_spin_unlock(&lock, key);
	}

k_mutex_unlock_return:
	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_mutex, unlock, mutex, 0);

	return 0;
}

#ifdef CONFIG_USERSPACE
static inline int z_vrfy_k_mutex_unlock(struct k_mutex *mutex)
{
	K_OOPS(K_SYSCALL_OBJ(mutex, K_OBJ_MUTEX));
	return z_impl_k_mutex_unlock(mutex);
}
#include <zephyr/syscalls/k_mutex_unlock_mrsh.c>
#endif /* CONFIG_USERSPACE */

#ifdef CONFIG_OBJ_CORE_MUTEX
static int init_mutex_obj_core_list(void)
{
	z_obj_type_init(&obj_type_mutex, K_OBJ_TYPE_MUTEX_ID,
			offsetof(struct k_mutex, obj_core));

	STRUCT_SECTION_FOREACH(k_mutex, mutex) {
		k_obj_core_init_and_link(K_OBJ_CORE(mutex), &obj_type_mutex);
	}

	return 0;
}

SYS_INIT(init_mutex_obj_core_list, PRE_KERNEL_1,
	 CONFIG_KERNEL_INIT_PRIORITY_OBJECTS);
#endif /* CONFIG_OBJ_CORE_MUTEX */
