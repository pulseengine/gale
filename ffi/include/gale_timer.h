/*
 * Gale Timer FFI — verified status counter arithmetic.
 *
 * These functions replace the status counter increment and
 * read-reset operations in kernel/timer.c.  The C shim passes
 * the current status value and receives the validated result:
 *   expire:     status -> status + 1 (checked)
 *   status_get: status -> 0 (returns old value)
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#ifndef GALE_TIMER_H
#define GALE_TIMER_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Validate timer init parameters.
 *
 * Period 0 = one-shot, period > 0 = periodic.  Always succeeds.
 *
 * @param period  Timer period in ticks.
 *
 * @return 0 (always OK).
 */
int32_t gale_timer_init_validate(uint32_t period);

/**
 * Record a timer expiry: checked status increment.
 *
 * Caller updates timer->status with *new_status on success.
 *
 * @param status     Current expiry count.
 * @param new_status Output: status + 1.
 *
 * @return 0 on success, -EOVERFLOW if status == UINT32_MAX.
 */
int32_t gale_timer_expire(uint32_t status,
                            uint32_t *new_status);

/**
 * Read and reset the status counter.
 *
 * Caller updates timer->status with *new_status (0) after this call.
 *
 * @param status     Current expiry count.
 * @param new_status Output: 0 (reset value).
 *
 * @return The old status value (number of expiries since last read).
 */
uint32_t gale_timer_status_get(uint32_t status,
                                 uint32_t *new_status);

#ifdef __cplusplus
}
#endif

#endif /* GALE_TIMER_H */
