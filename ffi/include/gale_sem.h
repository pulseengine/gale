/*
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Phase 1 FFI: verified count arithmetic for Zephyr's k_sem.
 *
 * These three functions replace the count arithmetic from kernel/sem.c.
 * All other semaphore logic (wait queue, scheduling, tracing, poll)
 * remains native Zephyr C.
 */

#ifndef GALE_SEM_H_
#define GALE_SEM_H_

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Validate semaphore initialization parameters.
 * Returns 0 if valid, -EINVAL if limit == 0 or initial_count > limit.
 *
 * Verified: P1 (0 <= count <= limit), P2 (limit > 0).
 */
int32_t gale_sem_count_init(uint32_t initial_count, uint32_t limit);

/**
 * Compute new count after give (signal) with no waiters.
 * Returns count + 1 if count < limit, else count (saturation).
 *
 * Verified: P3 (increment capped at limit), P9 (no overflow).
 */
uint32_t gale_sem_count_give(uint32_t count, uint32_t limit);

/**
 * Attempt to decrement count for take (acquire).
 * If *count > 0: decrements *count by 1, returns 0.
 * If *count == 0: leaves *count unchanged, returns -EBUSY.
 *
 * Verified: P5 (decrement by 1), P6 (-EBUSY), P9 (no underflow).
 */
int32_t gale_sem_count_take(uint32_t *count);

/* ---- Phase 2: Full Decision API ---- */

struct gale_sem_give_decision {
    uint8_t action;     /* 0=INCREMENT_COUNT, 1=WAKE_THREAD */
    uint32_t new_count;
};

#define GALE_SEM_ACTION_INCREMENT 0
#define GALE_SEM_ACTION_WAKE      1

struct gale_sem_give_decision gale_k_sem_give_decide(
    uint32_t count, uint32_t limit, uint32_t has_waiter);

struct gale_sem_take_decision {
    int32_t ret;
    uint32_t new_count;
    uint8_t action;     /* 0=RETURN_IMMEDIATELY, 1=PEND_CURRENT */
};

#define GALE_SEM_ACTION_RETURN 0
#define GALE_SEM_ACTION_PEND   1

struct gale_sem_take_decision gale_k_sem_take_decide(
    uint32_t count, uint32_t is_no_wait);

#ifdef __cplusplus
}
#endif

#endif /* GALE_SEM_H_ */
