/*
 * Gale Thread Lifecycle FFI — verified create/exit counting and priority
 * validation.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#ifndef GALE_THREAD_LIFECYCLE_H
#define GALE_THREAD_LIFECYCLE_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Validate thread creation: check count < MAX_THREADS and increment.
 *
 * @param count      Current active thread count.
 * @param new_count  Output: count + 1.
 *
 * @return 0 on success, -EAGAIN at capacity, -EINVAL on null pointer.
 */
int32_t gale_thread_create_validate(uint32_t count, uint32_t *new_count);

/**
 * Validate thread exit: check count > 0 and decrement.
 *
 * @param count      Current active thread count.
 * @param new_count  Output: count - 1.
 *
 * @return 0 on success, -EINVAL on underflow or null pointer.
 */
int32_t gale_thread_exit_validate(uint32_t count, uint32_t *new_count);

/**
 * Validate a thread priority value.
 *
 * @param priority  Proposed priority value.
 *
 * @return 0 if valid (< MAX_PRIORITY), -EINVAL if out of range.
 */
int32_t gale_thread_priority_validate(uint32_t priority);

#ifdef __cplusplus
}
#endif

#endif /* GALE_THREAD_LIFECYCLE_H */
