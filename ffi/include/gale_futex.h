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

#ifdef __cplusplus
}
#endif

#endif /* GALE_FUTEX_H */
