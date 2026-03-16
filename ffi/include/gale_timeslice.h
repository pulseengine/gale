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

#ifdef __cplusplus
}
#endif

#endif /* GALE_TIMESLICE_H */
