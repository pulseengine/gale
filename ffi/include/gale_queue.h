/*
 * Gale Queue FFI — verified unbounded queue count arithmetic.
 *
 * These functions replace the count tracking in kernel/queue.c.
 * The C shim tracks the number of data items enqueued, providing
 * overflow/underflow protection for append, prepend, and get.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#ifndef GALE_QUEUE_H
#define GALE_QUEUE_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Validate a queue append operation and compute new count.
 *
 * Caller enqueues data at the tail after this succeeds.
 *
 * @param count     Current element count.
 * @param new_count Output: count + 1.
 *
 * @return 0 on success, -EOVERFLOW if count would overflow u32.
 */
int32_t gale_queue_append_validate(uint32_t count,
                                    uint32_t *new_count);

/**
 * Validate a queue prepend operation and compute new count.
 *
 * Caller enqueues data at the head after this succeeds.
 *
 * @param count     Current element count.
 * @param new_count Output: count + 1.
 *
 * @return 0 on success, -EOVERFLOW if count would overflow u32.
 */
int32_t gale_queue_prepend_validate(uint32_t count,
                                     uint32_t *new_count);

/**
 * Validate a queue get operation and compute new count.
 *
 * Caller dequeues data from the head after this succeeds.
 *
 * @param count     Current element count.
 * @param new_count Output: count - 1.
 *
 * @return 0 on success, -EAGAIN if queue empty.
 */
int32_t gale_queue_get_validate(uint32_t count,
                                 uint32_t *new_count);

#ifdef __cplusplus
}
#endif

#endif /* GALE_QUEUE_H */
