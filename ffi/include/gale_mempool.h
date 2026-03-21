/*
 * Gale MemPool FFI — verified fixed-block pool allocation tracking.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#ifndef GALE_MEMPOOL_H
#define GALE_MEMPOOL_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Validate a mempool allocation: increment block count.
 *
 * @param allocated      Current allocated block count.
 * @param capacity       Total blocks in pool.
 * @param new_allocated  Output: allocated + 1.
 *
 * @return 0 on success, -ENOMEM if full, -EINVAL on null pointer.
 */
int32_t gale_mempool_alloc_validate(uint32_t allocated,
                                     uint32_t capacity,
                                     uint32_t *new_allocated);

/**
 * Validate a mempool free: decrement block count.
 *
 * @param allocated      Current allocated block count.
 * @param new_allocated  Output: allocated - 1.
 *
 * @return 0 on success, -EINVAL on underflow or null pointer.
 */
int32_t gale_mempool_free_validate(uint32_t allocated,
                                    uint32_t *new_allocated);

/* ---- Phase 2: Full Decision API ---- */

struct gale_mempool_alloc_decision {
    uint8_t  action;       /* 0=RETURN_PTR, 1=RETURN_NULL */
};

#define GALE_MEMPOOL_ACTION_RETURN_PTR   0
#define GALE_MEMPOOL_ACTION_RETURN_NULL  1

struct gale_mempool_alloc_decision gale_k_mempool_alloc_decide(
    uint32_t alloc_succeeded);

struct gale_mempool_free_decision {
    uint8_t  action;       /* 0=FREE_OK, 1=FREE_AND_RESCHEDULE */
};

#define GALE_MEMPOOL_ACTION_FREE_OK            0
#define GALE_MEMPOOL_ACTION_FREE_AND_RESCHEDULE 1

struct gale_mempool_free_decision gale_k_mempool_free_decide(
    uint32_t has_waiters);

#ifdef __cplusplus
}
#endif

#endif /* GALE_MEMPOOL_H */
