/*
 * Gale Timeout FFI — verified tick arithmetic and deadline tracking.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#ifndef GALE_TIMEOUT_H
#define GALE_TIMEOUT_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Schedule a timeout: compute absolute deadline from current tick + duration.
 *
 * @param current_tick  Current system tick.
 * @param duration      Relative timeout in ticks.
 * @param deadline      Output: absolute deadline.
 *
 * @return 0 on success, -EINVAL on overflow or null pointer.
 */
int32_t gale_timeout_add(uint64_t current_tick,
                          uint64_t duration,
                          uint64_t *deadline);

/**
 * Abort a pending timeout.
 *
 * @param active  1 if timeout is active, 0 if inactive.
 *
 * @return 0 on success (was active), -EINVAL if already inactive.
 */
int32_t gale_timeout_abort(uint32_t active);

/**
 * Advance tick and check if a timeout has expired.
 *
 * @param current_tick  Current system tick.
 * @param ticks         Ticks to advance.
 * @param deadline      Absolute deadline of this timeout.
 * @param active        1 if timeout is active.
 * @param new_tick      Output: advanced tick value.
 * @param fired         Output: 1 if expired, 0 otherwise.
 *
 * @return 0 on success, -EINVAL on overflow or null pointer.
 */
int32_t gale_timeout_announce(uint64_t current_tick,
                               uint64_t ticks,
                               uint64_t deadline,
                               uint32_t active,
                               uint64_t *new_tick,
                               uint32_t *fired);

#ifdef __cplusplus
}
#endif

#endif /* GALE_TIMEOUT_H */
