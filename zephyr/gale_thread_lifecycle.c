/*
 * Copyright (c) 2010-2014 Wind River Systems, Inc.
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale thread lifecycle — phase 2: Extract→Decide→Apply pattern.
 *
 * This is kernel/thread.c + sched.c thread management, rewritten so
 * that Rust validates parameters and decides state transitions.
 * Context setup, arch-specific init, TLS, naming, and scheduling
 * remain in Zephyr C.
 *
 * Verified operations (Verus proofs):
 *   gale_k_thread_create_decide — TH1 (priority), TH3 (stack), TH6 (count)
 *   gale_k_thread_abort_decide  — TH5 (no double-abort)
 *   gale_k_thread_join_decide   — deadlock detection
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

#include "gale_thread_lifecycle.h"

LOG_MODULE_DECLARE(os, CONFIG_KERNEL_LOG_LEVEL);

/*
 * gale_thread_lifecycle_create_check — validate thread creation parameters.
 *
 * Called from z_setup_new_thread (or a wrapper around k_thread_create)
 * to let Rust validate the safety-critical parameters before C proceeds
 * with arch-specific thread initialization.
 *
 * Pattern:
 *   Extract: C reads stack_size, priority, options, active_count
 *   Decide:  Rust validates and returns PROCEED or REJECT
 *   Apply:   C proceeds with z_setup_new_thread or returns error
 *
 * Returns 0 if creation should proceed, negative errno otherwise.
 */
int gale_thread_lifecycle_create_check(size_t stack_size, int prio,
				       uint32_t options,
				       uint32_t active_count)
{
	/* Extract: gather parameters for Rust decision */
	uint32_t ss = (stack_size > UINT32_MAX) ? UINT32_MAX
					        : (uint32_t)stack_size;
	uint32_t pri = (prio < 0) ? 0U : (uint32_t)prio;

	/* Decide: Rust validates stack_size, priority, options, count */
	struct gale_thread_create_decision d =
		gale_k_thread_create_decide(ss, pri, options, active_count);

	/* Apply: return Rust's decision */
	if (d.action == GALE_THREAD_ACTION_REJECT) {
		LOG_DBG("Gale: thread create rejected (ret=%d)", d.ret);
		return d.ret;
	}

	return 0;
}

/*
 * gale_thread_lifecycle_abort — decide and apply thread abort.
 *
 * Wraps z_thread_abort with Rust decision logic. Called where
 * k_thread_abort would normally be called.
 *
 * Pattern:
 *   Extract: C reads thread_state, is_essential from the thread
 *   Decide:  Rust decides ABORT/ALREADY_DEAD/PANIC
 *   Apply:   C executes the decision
 *
 * Note: The actual halt_thread / z_thread_halt logic stays in
 * sched.c — this function only wraps the guard/decision layer.
 */
void gale_thread_lifecycle_abort(struct k_thread *thread)
{
	/* Extract: read thread state flags */
	uint8_t state = thread->base.thread_state;
	uint32_t essential = z_is_thread_essential(thread) ? 1U : 0U;

	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_thread, abort, thread);

	/* Decide: Rust determines action */
	struct gale_thread_abort_decision d =
		gale_k_thread_abort_decide(state, essential);

	/* Apply */
	switch (d.action) {
	case GALE_THREAD_ABORT_ALREADY_DEAD:
		/* No-op: thread is already dead */
		break;

	case GALE_THREAD_ABORT_PANIC:
		/*
		 * Essential thread — abort it, then panic.
		 * This matches Zephyr's z_thread_abort behavior:
		 *   z_thread_halt(thread, key, true);
		 *   if (essential) { k_panic(); }
		 */
		z_thread_abort(thread);
		__ASSERT(false, "aborted essential thread %p", thread);
		k_panic();
		break;

	case GALE_THREAD_ABORT_PROCEED:
	default:
		/* Normal abort — delegate to Zephyr's z_thread_abort */
		z_thread_abort(thread);
		break;
	}

	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_thread, abort, thread);
}

/*
 * gale_thread_lifecycle_join — decide and apply thread join.
 *
 * Wraps z_impl_k_thread_join with Rust decision logic.
 *
 * Pattern:
 *   Extract: C reads is_dead, timeout mode, deadlock conditions
 *   Decide:  Rust decides RETURN (with code) or PEND
 *   Apply:   C returns immediately or pends on join queue
 *
 * The actual pending logic (z_pend_curr / add_to_waitq) stays in
 * sched.c — this wraps the decision layer.
 */
int gale_thread_lifecycle_join(struct k_thread *thread, k_timeout_t timeout)
{
	k_spinlock_key_t key;
	int ret;

	/* Extract: gather state for Rust decision */
	key = k_spin_lock(&_sched_spinlock);

	uint32_t is_dead = z_is_thread_dead(thread) ? 1U : 0U;
	uint32_t is_no_wait = K_TIMEOUT_EQ(timeout, K_NO_WAIT) ? 1U : 0U;
	uint32_t is_self_or_circular =
		(thread == _current ||
		 thread->base.pended_on == &_current->join_queue) ? 1U : 0U;

	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_thread, join, thread, timeout);

	/* Decide: Rust determines action */
	struct gale_thread_join_decision d =
		gale_k_thread_join_decide(is_dead, is_no_wait,
					  is_self_or_circular);

	/* Apply */
	if (d.action == GALE_THREAD_JOIN_RETURN) {
		ret = d.ret;
		SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_thread, join, thread,
						timeout, ret);
		k_spin_unlock(&_sched_spinlock, key);
		return ret;
	}

	/* PEND: block on join queue with timeout.
	 * Use z_pend_curr which is the public scheduler API for blocking. */
	SYS_PORT_TRACING_OBJ_FUNC_BLOCKING(k_thread, join, thread, timeout);
	ret = z_pend_curr(&_sched_spinlock, key, &thread->join_queue, timeout);
	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_thread, join, thread, timeout, ret);

	return ret;
}
