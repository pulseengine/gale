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

/* ---- Decision API for timer ---- */

struct gale_timer_expire_decision {
    uint32_t new_status;  /* status + 1 (saturates at UINT32_MAX) */
    uint8_t is_periodic;  /* 1 = periodic (period > 0), 0 = one-shot */
};

struct gale_timer_expire_decision gale_k_timer_expire_decide(
    uint32_t status, uint32_t period);

struct gale_timer_status_decision {
    uint32_t count;       /* old status value to return */
    uint32_t new_status;  /* always 0 (reset) */
};

struct gale_timer_status_decision gale_k_timer_status_decide(
    uint32_t status);

#ifdef __cplusplus
}
#endif

#endif /* GALE_TIMER_H */
