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

/* ---- Phase 2: Full Decision API ---- */

struct gale_lifo_put_decision {
    uint8_t action;     /* 0=PUT_OK, 1=WAKE_THREAD */
};

#define GALE_LIFO_PUT_OK    0
#define GALE_LIFO_PUT_WAKE  1

/**
 * Decide action for lifo put (queue_insert).
 *
 * C shim calls z_unpend_first_thread first, then passes whether
 * a waiter was found.  Rust decides the action.
 *
 * @param count       Current element count (unused — lifo is unbounded).
 * @param has_waiter  1 if a thread was unpended, 0 otherwise.
 *
 * @return Decision struct with action field.
 */
struct gale_lifo_put_decision gale_k_lifo_put_decide(
    uint32_t count, uint32_t has_waiter);

struct gale_lifo_get_decision {
    int32_t ret;        /* 0 (OK), -EBUSY (empty + no_wait) */
    uint8_t action;     /* 0=GET_OK, 1=PEND_CURRENT, 2=RETURN_NODATA */
};

#define GALE_LIFO_GET_OK      0
#define GALE_LIFO_GET_PEND    1
#define GALE_LIFO_GET_NODATA  2

/**
 * Decide action for lifo get (k_queue_get).
 *
 * @param count       Current element count.
 * @param is_no_wait  1 if K_NO_WAIT, 0 otherwise.
 *
 * @return Decision struct with ret and action fields.
 */
struct gale_lifo_get_decision gale_k_lifo_get_decide(
    uint32_t count, uint32_t is_no_wait);

#ifdef __cplusplus
}
#endif

#endif /* GALE_LIFO_H */
