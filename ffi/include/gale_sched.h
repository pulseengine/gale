/*
 * Gale Sched FFI — verified scheduler primitives.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#ifndef GALE_SCHED_H
#define GALE_SCHED_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Select the next thread to run (uniprocessor).
 *
 * @param runq_best_prio  Priority of best thread in run queue
 *                         (UINT32_MAX if empty).
 * @param idle_prio       Priority of idle thread.
 * @param best_prio       Output: selected thread's priority.
 *
 * @return 0 (selected runq best), 1 (selected idle), -EINVAL on null.
 */
int32_t gale_sched_next_up(uint32_t runq_best_prio,
                            uint32_t idle_prio,
                            uint32_t *best_prio);

/**
 * Check whether a candidate should preempt the current thread.
 *
 * @param current_is_cooperative  1 if current is cooperative.
 * @param candidate_is_metairq   1 if candidate is MetaIRQ.
 * @param swap_ok                1 if explicit yield allows swap.
 *
 * @return 1 (should preempt), 0 (should not).
 */
int32_t gale_sched_should_preempt(uint32_t current_is_cooperative,
                                   uint32_t candidate_is_metairq,
                                   uint32_t swap_ok);

/* ---- Phase 3: Sched Decision API ---- */

#define GALE_SCHED_SELECT_RUNQ              0
#define GALE_SCHED_SELECT_IDLE              1
#define GALE_SCHED_SELECT_METAIRQ_PREEMPTED 2

struct gale_sched_next_up_decision {
    uint8_t action;     /* 0=SELECT_RUNQ, 1=SELECT_IDLE, 2=SELECT_METAIRQ_PREEMPTED */
};

/**
 * Decide which thread to run next (uniprocessor).
 *
 * The C shim extracts boolean flags from kernel state, Rust decides
 * the scheduling policy, C applies the decision.
 *
 * @param has_runq_thread              1 if run queue has a best thread.
 * @param runq_best_is_metairq        1 if the runq best thread is MetaIRQ.
 * @param has_metairq_preempted       1 if a coop thread was preempted by MetaIRQ.
 * @param metairq_preempted_is_ready  1 if the preempted thread is still ready.
 *
 * @return Decision struct with action field.
 */
struct gale_sched_next_up_decision gale_k_sched_next_up_decide(
    uint32_t has_runq_thread,
    uint32_t runq_best_is_metairq,
    uint32_t has_metairq_preempted,
    uint32_t metairq_preempted_is_ready);

#define GALE_SCHED_PREEMPT    1
#define GALE_SCHED_NO_PREEMPT 0

struct gale_sched_preempt_decision {
    uint8_t should_preempt;    /* 1=preempt, 0=no preempt */
};

/**
 * Decide whether the candidate thread should preempt current.
 *
 * Mirrors kthread.h:should_preempt with Extract-Decide-Apply:
 *   1. swap_ok (yield) -> always preempt
 *   2. current prevented from running -> preempt
 *   3. current preemptible OR candidate MetaIRQ -> preempt
 *   4. otherwise -> no preempt (cooperative protection)
 *
 * @param is_cooperative        1 if current thread is cooperative.
 * @param candidate_is_metairq  1 if candidate is MetaIRQ.
 * @param swap_ok               1 if explicit yield allows swap.
 * @param current_is_prevented  1 if current is pended/suspended/dummy.
 *
 * @return Decision struct with should_preempt field.
 */
struct gale_sched_preempt_decision gale_k_sched_preempt_decide(
    uint32_t is_cooperative,
    uint32_t candidate_is_metairq,
    uint32_t swap_ok,
    uint32_t current_is_prevented);

#ifdef __cplusplus
}
#endif

#endif /* GALE_SCHED_H */
