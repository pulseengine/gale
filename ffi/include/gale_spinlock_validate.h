/*
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale spinlock validate FFI — verified spinlock ownership validation.
 *
 * These three functions replace the validation logic from
 * kernel/spinlock_validate.c.  All other spinlock logic (acquire,
 * release, IRQ save/restore) remains native Zephyr C.
 *
 * Verified properties (Verus proofs):
 *   SV1: owner encoding is injective
 *   SV2: lock_valid returns false iff lock held by same CPU
 *   SV3: unlock_valid returns true iff owner matches (cpu | thread)
 *   SV4: CPU ID is recoverable from encoded owner
 *   SV5: thread pointer is recoverable from encoded owner
 *   SV6: CPU ID fits within the mask
 */

#ifndef GALE_SPINLOCK_VALIDATE_H_
#define GALE_SPINLOCK_VALIDATE_H_

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Check whether acquiring the spinlock is valid.
 *
 * @param thread_cpu     The lock's current thread_cpu field (0 if free).
 * @param current_cpu_id The current CPU's ID.
 *
 * @return 1 if valid (lock is free or held by different CPU),
 *         0 if invalid (lock already held by same CPU — would deadlock).
 */
int32_t gale_spin_lock_valid(uintptr_t thread_cpu, uint32_t current_cpu_id);

/**
 * Check whether releasing the spinlock is valid.
 *
 * @param thread_cpu     The lock's current thread_cpu field.
 * @param current_cpu_id The current CPU's ID.
 * @param current_thread The current thread pointer (uintptr_t).
 *
 * @return 1 if valid (stored owner matches cpu | thread),
 *         0 if invalid (owner mismatch).
 */
int32_t gale_spin_unlock_valid(uintptr_t thread_cpu, uint32_t current_cpu_id,
                                uintptr_t current_thread);

/**
 * Compute the owner tag for a spinlock: cpu_id | thread_ptr.
 *
 * @param current_cpu_id The current CPU's ID.
 * @param current_thread The current thread pointer (uintptr_t).
 *
 * @return Encoded owner value (cpu_id | thread_ptr).
 */
uintptr_t gale_spin_lock_compute_owner(uint32_t current_cpu_id,
                                        uintptr_t current_thread);

#ifdef __cplusplus
}
#endif

#endif /* GALE_SPINLOCK_VALIDATE_H_ */
