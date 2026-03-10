/*
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * C header for the Gale verified semaphore FFI.
 * Generated from ffi/src/lib.rs — keep in sync.
 */

#ifndef GALE_SEM_H_
#define GALE_SEM_H_

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Opaque semaphore handle — stored inside struct k_sem.
 * Contains a pool index into the static Rust semaphore table.
 */
struct gale_sem {
	uint32_t pool_index;
};

/**
 * Result of gale_sem_give().
 *   kind 0 = incremented (no waiters, count went up)
 *   kind 1 = woke thread (a waiter was unblocked)
 *   kind 2 = saturated (count was already at limit)
 */
struct gale_give_result {
	uint32_t kind;
	uint32_t woken_thread_id;
	uint32_t woken_thread_priority;
};

/** Initialize a semaphore. Returns 0 on success. */
int32_t gale_sem_init(struct gale_sem *handle, uint32_t initial_count,
		      uint32_t limit);

/** Give (signal). Fills result struct. Returns 0 on success. */
int32_t gale_sem_give(const struct gale_sem *handle,
		      struct gale_give_result *result);

/** Non-blocking take. Returns 0 if acquired, -EBUSY if empty. */
int32_t gale_sem_try_take(const struct gale_sem *handle);

/** Enqueue a thread as waiter. Returns 1=acquired, 2=enqueued, 0=error. */
int32_t gale_sem_pend_thread(const struct gale_sem *handle,
			     uint32_t thread_id, uint32_t priority);

/** Reset: count → 0, returns number of woken waiters. */
uint32_t gale_sem_reset(const struct gale_sem *handle);

/** Get current count. */
uint32_t gale_sem_count_get(const struct gale_sem *handle);

/** Get limit. */
uint32_t gale_sem_limit_get(const struct gale_sem *handle);

/** Free a semaphore slot. */
void gale_sem_free(const struct gale_sem *handle);

#ifdef __cplusplus
}
#endif

#endif /* GALE_SEM_H_ */
