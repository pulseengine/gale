/*
 * Gale Lifo FFI — verified unbounded queue count arithmetic.
 *
 * These functions replace the count tracking for k_lifo
 * (LIFO ordering wrapper around k_queue) in kernel/queue.c.
 * The C shim tracks the number of data items enqueued.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#ifndef GALE_LIFO_H
#define GALE_LIFO_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Validate a lifo put operation and compute new count.
 *
 * Caller enqueues data at the head after this succeeds.
 *
 * @param count     Current element count.
 * @param new_count Output: count + 1.
 *
 * @return 0 on success, -EOVERFLOW if count would overflow u32.
 */
int32_t gale_lifo_put_validate(uint32_t count,
                                uint32_t *new_count);

/**
 * Validate a lifo get operation and compute new count.
 *
 * Caller dequeues data from the head after this succeeds.
 *
 * @param count     Current element count.
 * @param new_count Output: count - 1.
 *
 * @return 0 on success, -EAGAIN if lifo empty.
 */
int32_t gale_lifo_get_validate(uint32_t count,
                                uint32_t *new_count);

#ifdef __cplusplus
}
#endif

#endif /* GALE_LIFO_H */
