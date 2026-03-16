/*
 * Gale Poll FFI — verified poll event state machine and signal.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#ifndef GALE_POLL_H
#define GALE_POLL_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Initialize a poll event: set state to NOT_READY.
 *
 * @param event_type  Poll event type (K_POLL_TYPE_*).
 * @param state       Output: initial state (0 = NOT_READY).
 *
 * @return 0 on success, -EINVAL on null pointer.
 */
int32_t gale_poll_event_init(uint32_t event_type, uint32_t *state);

/**
 * Check if a semaphore condition is met for a poll event.
 *
 * @param event_type  Poll event type.
 * @param sem_count   Current semaphore count.
 *
 * @return 1 if condition met, 0 otherwise.
 */
int32_t gale_poll_check_sem(uint32_t event_type, uint32_t sem_count);

/**
 * Raise a poll signal: set signaled flag and result.
 *
 * @param signaled    Pointer to signaled flag (set to 1).
 * @param result      Pointer to result value (set to result_val).
 * @param result_val  Value to store in result.
 *
 * @return 0 on success, -EINVAL on null pointer.
 */
int32_t gale_poll_signal_raise(uint32_t *signaled,
                                int32_t *result,
                                int32_t result_val);

/**
 * Reset a poll signal: clear signaled flag.
 *
 * @param signaled  Pointer to signaled flag (set to 0).
 *
 * @return 0 on success, -EINVAL on null pointer.
 */
int32_t gale_poll_signal_reset(uint32_t *signaled);

#ifdef __cplusplus
}
#endif

#endif /* GALE_POLL_H */
