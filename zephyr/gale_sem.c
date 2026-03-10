/*
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale semaphore — C shim bridging Zephyr's k_sem API to the
 * formally verified Rust implementation.
 *
 * This file replaces kernel/sem.c when CONFIG_GALE_KERNEL_SEM=y.
 * It handles Zephyr-specific concerns (spinlocks, scheduling,
 * tracing, poll events) and delegates count/wait-queue logic
 * to the verified Rust code via the gale_sem FFI.
 *
 * Source mapping (Zephyr → Gale):
 *   z_impl_k_sem_init  → gale_sem_init      (verified: P1, P2)
 *   z_impl_k_sem_give  → gale_sem_give      (verified: P3, P4, P9)
 *   z_impl_k_sem_take  → gale_sem_try_take   (verified: P5, P6, P9)
 *   z_impl_k_sem_reset → gale_sem_reset      (verified: P8)
 */

#include <zephyr/kernel.h>
#include <zephyr/kernel_structs.h>
#include <zephyr/toolchain.h>
#include <wait_q.h>
#include <ksched.h>
#include <zephyr/init.h>
#include <zephyr/internal/syscall_handler.h>
#include <zephyr/tracing/tracing.h>
#include <zephyr/sys/check.h>

#include "gale_sem.h"

/*
 * System-wide spinlock — same pattern as original kernel/sem.c.
 * The Rust code is called under this lock, so the static mutable
 * pool access in Rust is safe (single-threaded).
 */
static struct k_spinlock lock;

#ifdef CONFIG_OBJ_CORE_SEM
static struct k_obj_type obj_type_sem;
#endif

/*
 * The gale_sem handle is embedded in the k_sem's wait_q field.
 * Since we replace the wait queue entirely, we repurpose those bytes.
 * _wait_q_t is at least sizeof(sys_dlist_t) = 2 pointers >= 8 bytes,
 * and struct gale_sem is 4 bytes, so this always fits.
 */
#define GALE_HANDLE(sem) ((struct gale_sem *)&(sem)->wait_q)

static inline bool handle_poll_events(struct k_sem *sem)
{
#ifdef CONFIG_POLL
	return z_handle_obj_poll_events(&sem->poll_events,
					K_POLL_STATE_SEM_AVAILABLE);
#else
	ARG_UNUSED(sem);
	return false;
#endif
}

int z_impl_k_sem_init(struct k_sem *sem, unsigned int initial_count,
		      unsigned int limit)
{
	int ret;

	CHECKIF(limit == 0U || initial_count > limit) {
		SYS_PORT_TRACING_OBJ_FUNC(k_sem, init, sem, -EINVAL);
		return -EINVAL;
	}

	ret = gale_sem_init(GALE_HANDLE(sem), initial_count, limit);
	if (ret != 0) {
		SYS_PORT_TRACING_OBJ_FUNC(k_sem, init, sem, ret);
		return ret;
	}

	/*
	 * Keep count/limit in the C struct too for k_sem_count_get()
	 * which is an inline in kernel.h and reads sem->count directly.
	 */
	sem->count = initial_count;
	sem->limit = limit;

	SYS_PORT_TRACING_OBJ_FUNC(k_sem, init, sem, 0);

#if defined(CONFIG_POLL)
	sys_dlist_init(&sem->poll_events);
#endif
	k_object_init(sem);

#ifdef CONFIG_OBJ_CORE_SEM
	k_obj_core_init_and_link(K_OBJ_CORE(sem), &obj_type_sem);
#endif

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
#endif

void z_impl_k_sem_give(struct k_sem *sem)
{
	k_spinlock_key_t key = k_spin_lock(&lock);
	struct gale_give_result gale_result;
	bool resched = false;

	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_sem, give, sem);

	gale_sem_give(GALE_HANDLE(sem), &gale_result);

	switch (gale_result.kind) {
	case 1: {
		/*
		 * Gale woke a thread from its internal wait queue.
		 * We need to find the corresponding Zephyr thread and
		 * ready it.  The thread_id stored in Gale maps to the
		 * Zephyr thread that was pended via z_pend_curr().
		 *
		 * For the initial integration, we use Zephyr's native
		 * wait queue (z_unpend_first_thread) in parallel.
		 * TODO: unify the wait queue so Gale is the single
		 * source of truth.
		 */
		struct k_thread *thread = z_unpend_first_thread(
			(_wait_q_t *)&sem->wait_q);
		if (thread != NULL) {
			arch_thread_return_value_set(thread, 0);
			z_ready_thread(thread);
		}
		resched = true;
		break;
	}
	case 0:
		/* Incremented — update the C-side shadow copy */
		sem->count = gale_sem_count_get(GALE_HANDLE(sem));
		resched = handle_poll_events(sem);
		break;
	case 2:
		/* Saturated — nothing to do */
		break;
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
#endif

int z_impl_k_sem_take(struct k_sem *sem, k_timeout_t timeout)
{
	int ret;

	__ASSERT(((arch_is_in_isr() == false) ||
		  K_TIMEOUT_EQ(timeout, K_NO_WAIT)), "");

	k_spinlock_key_t key = k_spin_lock(&lock);

	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_sem, take, sem, timeout);

	ret = gale_sem_try_take(GALE_HANDLE(sem));

	if (ret == 0) {
		/* Acquired — update C-side shadow */
		sem->count = gale_sem_count_get(GALE_HANDLE(sem));
		k_spin_unlock(&lock, key);
		goto out;
	}

	/* Count is zero — check timeout */
	if (K_TIMEOUT_EQ(timeout, K_NO_WAIT)) {
		k_spin_unlock(&lock, key);
		ret = -EBUSY;
		goto out;
	}

	SYS_PORT_TRACING_OBJ_FUNC_BLOCKING(k_sem, take, sem, timeout);

	/*
	 * Block the calling thread.  We use Zephyr's native z_pend_curr
	 * for thread management (scheduling, timeouts).
	 *
	 * TODO: also enqueue in Gale's wait queue for verified ordering.
	 */
	ret = z_pend_curr(&lock, key, (_wait_q_t *)&sem->wait_q, timeout);

out:
	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_sem, take, sem, timeout, ret);
	return ret;
}

void z_impl_k_sem_reset(struct k_sem *sem)
{
	k_spinlock_key_t key = k_spin_lock(&lock);
	bool resched = false;
	struct k_thread *thread;

	/* Wake all waiters via Zephyr's native wait queue */
	while (true) {
		thread = z_unpend_first_thread((_wait_q_t *)&sem->wait_q);
		if (thread == NULL) {
			break;
		}
		resched = true;
		arch_thread_return_value_set(thread, -EAGAIN);
		z_ready_thread(thread);
	}

	/* Reset Gale's verified state */
	gale_sem_reset(GALE_HANDLE(sem));
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
#endif

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
#endif
