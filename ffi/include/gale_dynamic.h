/*
 * Gale Dynamic FFI — verified dynamic thread pool tracking.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#ifndef GALE_DYNAMIC_H
#define GALE_DYNAMIC_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Validate a dynamic pool allocation: increment active count.
 *
 * @param active       Current active stack count.
 * @param max_threads  Maximum threads in pool.
 * @param new_active   Output: active + 1.
 *
 * @return 0 on success, -ENOMEM if full, -EINVAL on null pointer.
 */
int32_t gale_dynamic_alloc_validate(uint32_t active,
                                     uint32_t max_threads,
                                     uint32_t *new_active);

/**
 * Validate a dynamic pool free: decrement active count.
 *
 * @param active      Current active stack count.
 * @param new_active  Output: active - 1.
 *
 * @return 0 on success, -EINVAL on underflow or null pointer.
 */
int32_t gale_dynamic_free_validate(uint32_t active,
                                    uint32_t *new_active);

/* ---- Phase 2: Full Decision API ---- */

struct gale_dynamic_alloc_decision {
    uint8_t action;     /* 0=ALLOC_OK, 1=POOL_FULL */
    uint32_t new_active;
};

#define GALE_DYNAMIC_ACTION_ALLOC_OK   0
#define GALE_DYNAMIC_ACTION_POOL_FULL  1

/**
 * Decide whether a dynamic pool allocation can proceed.
 *
 * @param active       Current active stack count.
 * @param max_threads  Maximum threads in pool.
 *
 * @return Decision struct: action + new_active.
 */
struct gale_dynamic_alloc_decision gale_dynamic_alloc_decide(
    uint32_t active, uint32_t max_threads);

struct gale_dynamic_free_decision {
    uint8_t action;     /* 0=FREE_OK, 1=UNDERFLOW */
    uint32_t new_active;
};

#define GALE_DYNAMIC_ACTION_FREE_OK    0
#define GALE_DYNAMIC_ACTION_UNDERFLOW  1

/**
 * Decide whether a dynamic pool free can proceed.
 *
 * @param active  Current active stack count.
 *
 * @return Decision struct: action + new_active.
 */
struct gale_dynamic_free_decision gale_dynamic_free_decide(uint32_t active);

#ifdef __cplusplus
}
#endif

#endif /* GALE_DYNAMIC_H */
