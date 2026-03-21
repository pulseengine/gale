/*
 * Gale Timeslice FFI — verified tick accounting for preemptive scheduling.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#ifndef GALE_TIMESLICE_H
#define GALE_TIMESLICE_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Reset the time slice counter to its maximum value.
 *
 * @param slice_max_ticks  Configured time-slice size in ticks.
 * @param new_ticks        Output: reset value (= slice_max_ticks).
 *
 * @return 0 on success, -EINVAL on null pointer.
 */
int32_t gale_timeslice_reset(uint32_t slice_max_ticks, uint32_t *new_ticks);

/**
 * Consume one tick of the time slice.
 *
 * @param slice_ticks  Current remaining ticks.
 * @param new_ticks    Output: decremented value.
 * @param expired      Output: 1 if expired (reached 0), 0 otherwise.
 *
 * @return 0 on success, -EINVAL on null pointer.
 */
int32_t gale_timeslice_tick(uint32_t slice_ticks,
                             uint32_t *new_ticks,
                             uint32_t *expired);

/* ---- Phase 2: Full Decision API ---- */

struct gale_timeslice_tick_decision {
    uint8_t action;      /* 0=NO_YIELD, 1=YIELD */
    uint32_t new_ticks;  /* reset to slice_ticks on yield, else ticks_remaining */
};

#define GALE_TIMESLICE_ACTION_NO_YIELD 0
#define GALE_TIMESLICE_ACTION_YIELD    1

/**
 * Decide whether the current thread should yield its time slice.
 *
 * Extract-Decide-Apply: C extracts slice state from per-CPU arrays and
 * thread flags, Rust decides if the thread should be preempted.
 *
 * @param ticks_remaining  Ticks left (0 = expired).
 * @param slice_ticks      Configured slice size (0 = no slicing).
 * @param is_cooperative   1 if thread is cooperative, 0 otherwise.
 *
 * @return Decision: action=YIELD when expired and preemptible.
 */
struct gale_timeslice_tick_decision gale_k_timeslice_tick_decide(
    uint32_t ticks_remaining,
    uint32_t slice_ticks,
    uint32_t is_cooperative);

#ifdef __cplusplus
}
#endif

#endif /* GALE_TIMESLICE_H */
