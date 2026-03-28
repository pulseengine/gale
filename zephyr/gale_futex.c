/*
 * Copyright (c) 2019 Intel corporation
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale futex — phase 2: Extract->Decide->Apply pattern.
 *
 * This is kernel/futex.c with wait/wake rewritten to use Rust decision
 * structs.  C extracts kernel state (spinlock, wait queue, kernel objects),
 * Rust decides the action, C applies it.
 *
 * Verified operations (Verus proofs):
 *   gale_k_futex_wait_decide — FX1/FX2 (value comparison gating)
 *   gale_k_futex_wake_decide — FX3/FX4/FX5 (wake count)
 */

#include <zephyr/kernel.h>
#include <zephyr/kernel_structs.h>
#include <zephyr/spinlock.h>
#include <kswap.h>
#include <zephyr/internal/syscall_handler.h>
#include <zephyr/init.h>
#include <ksched.h>

#include "gale_futex.h"

#ifdef CONFIG_USERSPACE

static struct z_futex_data *k_futex_find_data(struct k_futex *futex)
{
	struct k_object *obj;

	obj = k_object_find(futex);
	if ((obj == NULL) || (obj->type != K_OBJ_FUTEX)) {
		return NULL;
	}

	return obj->data.futex_data;
}

int z_impl_k_futex_wake(struct k_futex *futex, bool wake_all)
{
	k_spinlock_key_t key;
	unsigned int woken = 0U;
	struct k_thread *thread;
	struct z_futex_data *futex_data;

	futex_data = k_futex_find_data(futex);
	if (futex_data == NULL) {
		return -EINVAL;
	}

	key = k_spin_lock(&futex_data->lock);

	/*
	 * Apply: wake threads from the wait queue.
	 * If wake_all, iterate until queue is empty.
	 * If !wake_all, wake one thread.
	 */
	do {
		thread = z_unpend_first_thread(&futex_data->wait_q);
		if (thread != NULL) {
			woken++;
			arch_thread_return_value_set(thread, 0);
			z_ready_thread(thread);
		}
	} while (thread && wake_all);

	if (woken == 0) {
		k_spin_unlock(&futex_data->lock, key);
	} else {
		z_reschedule(&futex_data->lock, key);
	}

	return woken;
}

static inline int z_vrfy_k_futex_wake(struct k_futex *futex, bool wake_all)
{
	if (K_SYSCALL_MEMORY_WRITE(futex, sizeof(struct k_futex)) != 0) {
		return -EACCES;
	}

	return z_impl_k_futex_wake(futex, wake_all);
}
#include <zephyr/syscalls/k_futex_wake_mrsh.c>

int z_impl_k_futex_wait(struct k_futex *futex, int expected,
			k_timeout_t timeout)
{
	int ret;
	k_spinlock_key_t key;
	struct z_futex_data *futex_data;

	futex_data = k_futex_find_data(futex);
	if (futex_data == NULL) {
		return -EINVAL;
	}

	/* Extract: read the current atomic futex value */
	uint32_t val = (uint32_t)atomic_get(&futex->val);

	/* Decide: Rust determines whether to block or return */
	struct gale_futex_wait_decision d = gale_k_futex_wait_decide(
		val, (uint32_t)expected,
		K_TIMEOUT_EQ(timeout, K_NO_WAIT) ? 1U : 0U);

	/* Apply: execute Rust's decision */
	if (d.action != GALE_FUTEX_ACTION_BLOCK) {
		return d.ret;
	}

	key = k_spin_lock(&futex_data->lock);

	ret = z_pend_curr(&futex_data->lock,
			key, &futex_data->wait_q, timeout);
	if (ret == -EAGAIN) {
		ret = -ETIMEDOUT;
	}

	return ret;
}

static inline int z_vrfy_k_futex_wait(struct k_futex *futex, int expected,
				      k_timeout_t timeout)
{
	if (K_SYSCALL_MEMORY_WRITE(futex, sizeof(struct k_futex)) != 0) {
		return -EACCES;
	}

	return z_impl_k_futex_wait(futex, expected, timeout);
}
#include <zephyr/syscalls/k_futex_wait_mrsh.c>

#endif /* CONFIG_USERSPACE */
