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

#ifdef __cplusplus
}
#endif

#endif /* GALE_SCHED_H */
