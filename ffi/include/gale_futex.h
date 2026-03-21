/*
 * Gale Futex FFI — verified fast userspace mutex value comparison.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#ifndef GALE_FUTEX_H
#define GALE_FUTEX_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Check if a futex wait should block.
 *
 * @param val       Current futex value.
 * @param expected  Expected value.
 *
 * @return 0 if val == expected (block), -EAGAIN if mismatch.
 */
int32_t gale_futex_wait_check(uint32_t val, uint32_t expected);

/**
 * Validate futex wake and compute remaining waiters.
 *
 * @param num_waiters  Current number of waiting threads.
 * @param wake_all     1 to wake all, 0 to wake at most 1.
 * @param woken        Output: number of threads woken.
 * @param remaining    Output: remaining waiters after wake.
 *
 * @return 0 on success, -EINVAL on null pointer.
 */
int32_t gale_futex_wake(uint32_t num_waiters,
                         uint32_t wake_all,
                         uint32_t *woken,
                         uint32_t *remaining);

/* ---- Phase 2: Full Decision API ---- */

struct gale_futex_wait_decision {
    uint8_t action;     /* 0=BLOCK (pend on wait queue), 1=RETURN_EAGAIN */
    int32_t ret;        /* 0 if blocking, -EAGAIN/-ETIMEDOUT if not */
};

#define GALE_FUTEX_ACTION_BLOCK        0
#define GALE_FUTEX_ACTION_RETURN_EAGAIN 1

struct gale_futex_wait_decision gale_k_futex_wait_decide(
    uint32_t val, uint32_t expected, uint32_t is_no_wait);

struct gale_futex_wake_decision {
    uint32_t wake_limit;  /* maximum number of threads to wake */
};

struct gale_futex_wake_decision gale_k_futex_wake_decide(
    uint32_t num_waiters, uint32_t wake_all);

#ifdef __cplusplus
}
#endif

#endif /* GALE_FUTEX_H */
