/*
 * Copyright (c) 2010-2016 Wind River Systems, Inc.
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale semaphore — phase 1: verified count arithmetic.
 *
 * This is kernel/sem.c with the count operations replaced by calls
 * to the formally verified Rust implementation.  Wait queue,
 * scheduling, tracing, and poll handling remain native Zephyr.
 *
 * Verified operations (Verus + Rocq proofs):
 *   gale_sem_count_give  — P3 (increment capped at limit), P9 (no overflow)
 *   gale_sem_count_take  — P5 (decrement by 1), P6 (-EBUSY), P9 (no underflow)
 *   gale_sem_count_init  — P1 (0 <= count <= limit), P2 (limit > 0)
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

#include "gale_sem.h"

static struct k_spinlock lock;

#ifdef CONFIG_OBJ_CORE_SEM
static struct k_obj_type obj_type_sem;
#endif /* CONFIG_OBJ_CORE_SEM */

static inline bool handle_poll_events(struct k_sem *sem)
{
#ifdef CONFIG_POLL
	return z_handle_obj_poll_events(&sem->poll_events, K_POLL_STATE_SEM_AVAILABLE);
#else
	ARG_UNUSED(sem);
	return false;
#endif /* CONFIG_POLL */
}

int z_impl_k_sem_init(struct k_sem *sem, unsigned int initial_count,
		      unsigned int limit)
{
	/*
	 * Validated by Gale: P1 (0 <= count <= limit), P2 (limit > 0).
	 */
	if (gale_sem_count_init(initial_count, limit) != 0) {
		SYS_PORT_TRACING_OBJ_FUNC(k_sem, init, sem, -EINVAL);
		return -EINVAL;
	}

	sem->count = initial_count;
	sem->limit = limit;

	SYS_PORT_TRACING_OBJ_FUNC(k_sem, init, sem, 0);

	z_waitq_init(&sem->wait_q);
#if defined(CONFIG_POLL)
	sys_dlist_init(&sem->poll_events);
#endif /* CONFIG_POLL */
	k_object_init(sem);

#ifdef CONFIG_OBJ_CORE_SEM
	k_obj_core_init_and_link(K_OBJ_CORE(sem), &obj_type_sem);
#endif /* CONFIG_OBJ_CORE_SEM */

	return 0;
}

#ifdef CONFIG_USERSPACE
int z_vrfy_k_sem_init(struct k_sem *sem, unsigned int initial_count,
		      unsigned int limit)
{
	K_OOPS(K_SYSCALL_OBJ_INIT(sem, K_OBJ_SEM));
	return z_impl_k_sem_init(sem, initial_count, limit);
}
#include <zephyr/syscalls/k_sem_init_mrsh.c>
#endif /* CONFIG_USERSPACE */

void z_impl_k_sem_give(struct k_sem *sem)
{
	k_spinlock_key_t key = k_spin_lock(&lock);
	struct k_thread *thread;
	bool resched;

	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_sem, give, sem);

	thread = z_unpend_first_thread(&sem->wait_q);

	if (unlikely(thread != NULL)) {
		arch_thread_return_value_set(thread, 0);
		z_ready_thread(thread);
		resched = true;
	} else {
		/*
		 * Verified by Gale: P3 (increment capped at limit),
		 * P9 (no arithmetic overflow).
		 */
		sem->count = gale_sem_count_give(sem->count, sem->limit);
		resched = handle_poll_events(sem);
	}

	if (unlikely(resched)) {
		z_reschedule(&lock, key);
	} else {
		k_spin_unlock(&lock, key);
	}

	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_sem, give, sem);
}

#ifdef CONFIG_USERSPACE
static inline void z_vrfy_k_sem_give(struct k_sem *sem)
{
	K_OOPS(K_SYSCALL_OBJ(sem, K_OBJ_SEM));
	z_impl_k_sem_give(sem);
}
#include <zephyr/syscalls/k_sem_give_mrsh.c>
#endif /* CONFIG_USERSPACE */

int z_impl_k_sem_take(struct k_sem *sem, k_timeout_t timeout)
{
	int ret;

	__ASSERT(((arch_is_in_isr() == false) ||
		  K_TIMEOUT_EQ(timeout, K_NO_WAIT)), "");

	k_spinlock_key_t key = k_spin_lock(&lock);

	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_sem, take, sem, timeout);

	/*
	 * Verified by Gale: P5 (decrement by 1 when count > 0),
	 * P6 (-EBUSY when count == 0), P9 (no underflow).
	 */
	ret = gale_sem_count_take(&sem->count);

	if (ret == 0) {
		k_spin_unlock(&lock, key);
		goto out;
	}

	if (K_TIMEOUT_EQ(timeout, K_NO_WAIT)) {
		k_spin_unlock(&lock, key);
		ret = -EBUSY;
		goto out;
	}

	SYS_PORT_TRACING_OBJ_FUNC_BLOCKING(k_sem, take, sem, timeout);

	ret = z_pend_curr(&lock, key, &sem->wait_q, timeout);

out:
	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_sem, take, sem, timeout, ret);

	return ret;
}

void z_impl_k_sem_reset(struct k_sem *sem)
{
	struct k_thread *thread;
	k_spinlock_key_t key = k_spin_lock(&lock);
	bool resched = false;

	while (true) {
		thread = z_unpend_first_thread(&sem->wait_q);
		if (thread == NULL) {
			break;
		}
		resched = true;
		arch_thread_return_value_set(thread, -EAGAIN);
		z_ready_thread(thread);
	}
	sem->count = 0;

	SYS_PORT_TRACING_OBJ_FUNC(k_sem, reset, sem);

	resched = handle_poll_events(sem) || resched;

	if (resched) {
		z_reschedule(&lock, key);
	} else {
		k_spin_unlock(&lock, key);
	}
}

#ifdef CONFIG_USERSPACE
static inline int z_vrfy_k_sem_take(struct k_sem *sem, k_timeout_t timeout)
{
	K_OOPS(K_SYSCALL_OBJ(sem, K_OBJ_SEM));
	return z_impl_k_sem_take(sem, timeout);
}
#include <zephyr/syscalls/k_sem_take_mrsh.c>

static inline void z_vrfy_k_sem_reset(struct k_sem *sem)
{
	K_OOPS(K_SYSCALL_OBJ(sem, K_OBJ_SEM));
	z_impl_k_sem_reset(sem);
}
#include <zephyr/syscalls/k_sem_reset_mrsh.c>

static inline unsigned int z_vrfy_k_sem_count_get(struct k_sem *sem)
{
	K_OOPS(K_SYSCALL_OBJ(sem, K_OBJ_SEM));
	return z_impl_k_sem_count_get(sem);
}
#include <zephyr/syscalls/k_sem_count_get_mrsh.c>

#endif /* CONFIG_USERSPACE */

#ifdef CONFIG_OBJ_CORE_SEM
static int init_sem_obj_core_list(void)
{
	z_obj_type_init(&obj_type_sem, K_OBJ_TYPE_SEM_ID,
			offsetof(struct k_sem, obj_core));

	STRUCT_SECTION_FOREACH(k_sem, sem) {
		k_obj_core_init_and_link(K_OBJ_CORE(sem), &obj_type_sem);
	}

	return 0;
}

SYS_INIT(init_sem_obj_core_list, PRE_KERNEL_1,
	 CONFIG_KERNEL_INIT_PRIORITY_OBJECTS);
#endif /* CONFIG_OBJ_CORE_SEM */
