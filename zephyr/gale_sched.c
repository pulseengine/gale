/*
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale sched — phase 3: Extract-Decide-Apply pattern.
 *
 * This C shim provides the glue between Zephyr's scheduler
 * and the formally verified Rust FFI. Run queue data structures,
 * thread state transitions, wait queues, and SMP IPI remain in Zephyr.
 *
 * Gale replaces the scheduling *policy* decisions with verified Rust:
 *   - next_up: which thread to run next (runq best, idle, or metairq preempted)
 *   - should_preempt: whether a candidate should preempt current
 *
 * Verified operations (Verus proofs):
 *   gale_k_sched_next_up_decide   — SC5 (highest-priority), SC7 (idle fallback)
 *   gale_k_sched_preempt_decide   — SC6 (cooperative protection)
 */

#include <zephyr/kernel.h>
#include <zephyr/kernel_structs.h>
#include <ksched.h>
#include <kthread.h>
#include <priority_q.h>

#include "gale_sched.h"

/*
 * ---- Wrappers using the decision struct pattern ----
 *
 * These functions demonstrate the Extract-Decide-Apply pattern for the
 * scheduler. They are available for use by Gale-aware scheduler code.
 *
 * The upstream sched.c still uses the Phase 1 scalar API
 * (gale_sched_next_up / gale_sched_should_preempt) for compatibility.
 * These Phase 3 wrappers provide richer decisions (MetaIRQ preemption
 * tracking, prevented-from-running state) and can replace the upstream
 * functions when CONFIG_GALE_KERNEL_SCHED is enabled.
 */

/**
 * gale_sched_do_next_up - Select the next thread using Rust decision struct.
 *
 * Extract: read run queue best, idle thread, metairq preempted state.
 * Decide:  call gale_k_sched_next_up_decide (Rust).
 * Apply:   return the selected thread pointer.
 *
 * This mirrors sched.c:next_up() for the uniprocessor path.
 * Run queue management, MetaIRQ bookkeeping, and thread state remain in C.
 */
struct k_thread *gale_sched_do_next_up(void)
{
	/* --- Extract --- */
	struct k_thread *runq_thread = _priq_run_best(&_kernel.ready_q.runq);
	struct k_thread *idle_thread = _current_cpu->idle_thread;

	uint32_t has_runq_thread = (runq_thread != NULL) ? 1U : 0U;
	uint32_t runq_best_is_metairq = 0U;

	if (runq_thread != NULL) {
		runq_best_is_metairq = thread_is_metairq(runq_thread) ? 1U : 0U;
	}

	uint32_t has_metairq_preempted = 0U;
	uint32_t metairq_preempted_is_ready = 0U;

#if (CONFIG_NUM_METAIRQ_PRIORITIES > 0)
	struct k_thread *mirqp = _current_cpu->metairq_preempted;

	if (mirqp != NULL) {
		has_metairq_preempted = 1U;
		metairq_preempted_is_ready = z_is_thread_ready(mirqp) ? 1U : 0U;
	}
#endif

	/* --- Decide --- */
	struct gale_sched_next_up_decision d = gale_k_sched_next_up_decide(
		has_runq_thread,
		runq_best_is_metairq,
		has_metairq_preempted,
		metairq_preempted_is_ready);

	/* --- Apply --- */
	switch (d.action) {
#if (CONFIG_NUM_METAIRQ_PRIORITIES > 0)
	case GALE_SCHED_SELECT_METAIRQ_PREEMPTED:
		return _current_cpu->metairq_preempted;
#endif
	case GALE_SCHED_SELECT_RUNQ:
		return runq_thread;
	case GALE_SCHED_SELECT_IDLE:
	default:
		/*
		 * If Rust said idle but metairq_preempted was not ready,
		 * clear the stale pointer (the C side effect).
		 */
#if (CONFIG_NUM_METAIRQ_PRIORITIES > 0)
		if (has_metairq_preempted != 0U && metairq_preempted_is_ready == 0U) {
			_current_cpu->metairq_preempted = NULL;
		}
#endif
		return idle_thread;
	}
}

/**
 * gale_sched_do_should_preempt - Decide preemption using Rust decision struct.
 *
 * Extract: read current thread state (cooperative, prevented from running).
 * Decide:  call gale_k_sched_preempt_decide (Rust).
 * Apply:   return boolean result.
 *
 * This mirrors kthread.h:should_preempt().
 */
bool gale_sched_do_should_preempt(struct k_thread *candidate, int preempt_ok)
{
	/* --- Extract --- */
	uint32_t is_cooperative = !thread_is_preemptible(_current) ? 1U : 0U;
	uint32_t candidate_is_metairq = thread_is_metairq(candidate) ? 1U : 0U;
	uint32_t swap_ok = (preempt_ok != 0) ? 1U : 0U;
	uint32_t current_is_prevented =
		z_is_thread_prevented_from_running(_current) ? 1U : 0U;

	/* --- Decide --- */
	struct gale_sched_preempt_decision d = gale_k_sched_preempt_decide(
		is_cooperative,
		candidate_is_metairq,
		swap_ok,
		current_is_prevented);

	/* --- Apply --- */
	return d.should_preempt != 0U;
}
