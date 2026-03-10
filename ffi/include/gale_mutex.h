/*
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Phase 1 FFI: verified state machine validation for Zephyr's k_mutex.
 *
 * These two functions replace the ownership checks and lock_count
 * arithmetic from kernel/mutex.c.  All other mutex logic (wait queue,
 * scheduling, priority inheritance, tracing) remains native Zephyr C.
 */

#ifndef GALE_MUTEX_H_
#define GALE_MUTEX_H_

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Return code: mutex still held (reentrant unlock, lock_count decremented).
 */
#define GALE_MUTEX_RELEASED 1

/**
 * Return code: mutex fully unlocked (caller should check waiters).
 */
#define GALE_MUTEX_UNLOCKED 0

/**
 * Validate a mutex lock attempt.
 *
 * Replaces mutex.c:121-129 (lock_count/owner checks + lock_count++).
 *
 * @param lock_count       Current mutex->lock_count value.
 * @param owner_is_null    1 if mutex->owner == NULL, 0 otherwise.
 * @param owner_is_current 1 if mutex->owner == _current, 0 otherwise.
 * @param new_lock_count   Output: new lock_count value on success.
 *
 * @return 0 (OK)    Lock acquired. Caller should set mutex->owner = _current
 *                    and mutex->lock_count = *new_lock_count.
 * @return -EBUSY    Mutex held by a different thread.
 * @return -EINVAL   NULL pointer or overflow protection.
 *
 * Verified: M3 (acquire), M4 (reentrant), M5 (contended), M10 (no overflow).
 */
int32_t gale_mutex_lock_validate(uint32_t lock_count,
				 uint32_t owner_is_null,
				 uint32_t owner_is_current,
				 uint32_t *new_lock_count);

/**
 * Validate a mutex unlock attempt.
 *
 * Replaces mutex.c:238-268 (owner checks + lock_count--).
 *
 * @param lock_count       Current mutex->lock_count value.
 * @param owner_is_null    1 if mutex->owner == NULL, 0 otherwise.
 * @param owner_is_current 1 if mutex->owner == _current, 0 otherwise.
 * @param new_lock_count   Output: new lock_count value on success.
 *
 * @return GALE_MUTEX_RELEASED (1) Still held (reentrant). Caller should set
 *                                 mutex->lock_count = *new_lock_count.
 * @return GALE_MUTEX_UNLOCKED (0) Fully unlocked. Caller should check
 *                                 wait queue for ownership transfer.
 * @return -EINVAL                 Mutex not locked (no owner).
 * @return -EPERM                  Current thread is not the owner.
 *
 * Verified: M6a (EINVAL), M6b (EPERM), M7 (reentrant), M10 (no underflow).
 */
int32_t gale_mutex_unlock_validate(uint32_t lock_count,
				   uint32_t owner_is_null,
				   uint32_t owner_is_current,
				   uint32_t *new_lock_count);

#ifdef __cplusplus
}
#endif

#endif /* GALE_MUTEX_H_ */
