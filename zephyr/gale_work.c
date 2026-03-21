/*
 * Copyright (c) 2020 Nordic Semiconductor ASA
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale work — phase 2: Extract->Decide->Apply pattern.
 *
 * This C shim provides Gale-aware replacements for the flag decision
 * logic in kernel/work.c.  Queue management, scheduling, handler
 * dispatch, and delayable work remain in upstream Zephyr.
 *
 * Two internal functions from work.c are replaced:
 *   submit_to_queue_locked — flag logic replaced by gale_k_work_submit_decide
 *   cancel_async_locked    — flag logic replaced by gale_k_work_cancel_decide
 *
 * Verified operations (Verus proofs):
 *   gale_k_work_submit_decide — WK2 (queue), WK3 (reject cancel), WK4 (idempotent)
 *   gale_k_work_cancel_decide — WK5 (clear queued, set canceling)
 */

#include <zephyr/kernel.h>
#include <zephyr/kernel_structs.h>
#include <zephyr/spinlock.h>
#include <zephyr/sys/slist.h>
#include <ksched.h>
#include <errno.h>

#include "gale_work.h"

/* -----------------------------------------------------------------
 * Gale submit_to_queue_locked — Extract->Decide->Apply
 *
 * Replaces the flag decision logic from work.c:submit_to_queue_locked.
 * Queue submission (sys_slist_append, notify) stays in C.
 *
 * Must be invoked with the work subsystem spinlock held.
 *
 * @param work   the work structure to be submitted
 * @param queuep pointer to a queue reference (may be updated)
 *
 * @retval 0 if work was already submitted to a queue
 * @retval 1 if work was not submitted and has been queued
 * @retval 2 if work was running and has been queued to its running queue
 * @retval -EBUSY if canceling or submission was rejected by queue
 * @retval -EINVAL if no queue is provided
 * @retval -ENODEV if the queue is not started
 * ----------------------------------------------------------------- */
int gale_submit_to_queue_locked(struct k_work *work,
				struct k_work_q **queuep)
{
	/* Extract: read flags under spinlock */
	uint32_t flags = work->flags;
	uint8_t is_queued  = (flags & K_WORK_QUEUED)  ? 1U : 0U;
	uint8_t is_running = (flags & K_WORK_RUNNING) ? 1U : 0U;

	/* Decide: Rust determines action based on flag state */
	struct gale_work_submit_decision d =
		gale_k_work_submit_decide((uint8_t)(flags & 0xFFU),
					  is_queued, is_running);

	/* Apply: execute Rust's decision */
	if (d.action == GALE_WORK_SUBMIT_ALREADY) {
		/* Already queued — no-op */
		return d.ret;
	}

	if (d.action == GALE_WORK_SUBMIT_REJECT) {
		/* Canceling — rejected */
		*queuep = NULL;
		return d.ret;
	}

	/*
	 * QUEUE or REQUEUE: the item needs to be placed on a queue.
	 *
	 * If re-queuing a running item, force the queue to be the one
	 * it's currently running on (prevents handler re-entrancy).
	 */
	if (d.action == GALE_WORK_SUBMIT_REQUEUE) {
		__ASSERT_NO_MSG(work->queue != NULL);
		*queuep = work->queue;
	}

	/* Fall back to last-used queue if none specified */
	if (*queuep == NULL) {
		*queuep = work->queue;
	}

	/* Validate queue state (C-side: queue lifecycle checks) */
	if (*queuep == NULL) {
		return -EINVAL;
	}

	/* Check queue started / draining / plugged (same as upstream) */
	bool chained = (_current == (*queuep)->thread_id) && !k_is_in_isr();
	bool draining = ((*queuep)->flags & BIT(K_WORK_QUEUE_DRAIN_BIT)) != 0U;
	bool plugged  = ((*queuep)->flags & BIT(K_WORK_QUEUE_PLUGGED_BIT)) != 0U;

	if (!((*queuep)->flags & BIT(K_WORK_QUEUE_STARTED_BIT))) {
		*queuep = NULL;
		return -ENODEV;
	}

	if (draining && !chained) {
		*queuep = NULL;
		return -EBUSY;
	}

	if (plugged && !draining) {
		*queuep = NULL;
		return -EBUSY;
	}

	/* Queue the work item */
	sys_slist_append(&(*queuep)->pending, &work->node);

	/* Apply Rust's flag decision */
	work->flags = (work->flags & ~0xFFU) | d.new_flags;
	work->queue = *queuep;

	/* Notify queue thread */
	(void)z_sched_wake(&(*queuep)->notifyq, 0, NULL);

	return d.ret;
}

/* -----------------------------------------------------------------
 * Gale cancel_async_locked — Extract->Decide->Apply
 *
 * Replaces the flag decision logic from work.c:cancel_async_locked.
 * Queue removal (sys_slist_find_and_remove) stays in C.
 *
 * Must be invoked with the work subsystem spinlock held.
 *
 * @param work the work item to be canceled
 *
 * @return busy status (0 = idle, nonzero = still busy/canceling)
 * ----------------------------------------------------------------- */
int gale_cancel_async_locked(struct k_work *work)
{
	/* Extract: read flags under spinlock */
	uint32_t flags = work->flags;
	uint8_t is_queued  = (flags & K_WORK_QUEUED)  ? 1U : 0U;
	uint8_t is_running = (flags & K_WORK_RUNNING) ? 1U : 0U;

	/* Decide: Rust determines action based on flag state */
	struct gale_work_cancel_decision d =
		gale_k_work_cancel_decide((uint8_t)(flags & 0xFFU),
					  is_queued, is_running);

	/* Apply: execute Rust's decision */
	if (d.action == GALE_WORK_CANCEL_DEQUEUE) {
		/* Remove from the queue (C-side: linked list manipulation) */
		if (work->queue != NULL) {
			(void)sys_slist_find_and_remove(
				&work->queue->pending, &work->node);
		}
	}

	/* Write back the updated flags */
	work->flags = (work->flags & ~0xFFU) | d.new_flags;

	return (int)d.busy;
}
