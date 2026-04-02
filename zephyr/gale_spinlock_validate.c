/*
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale spinlock validate — verified ownership validation.
 *
 * This C shim replaces the validation functions from
 * kernel/spinlock_validate.c with calls to the Gale Rust FFI.
 *
 * The three replaced functions:
 *   z_spin_lock_valid     -> gale_spin_lock_valid
 *   z_spin_unlock_valid   -> gale_spin_unlock_valid
 *   z_spin_lock_set_owner -> gale_spin_lock_compute_owner
 *
 * Verified properties (Verus proofs):
 *   SV1: owner encoding is injective
 *   SV2: lock_valid detects same-CPU deadlock
 *   SV3: unlock_valid checks owner match
 *   SV4/SV5: CPU and thread recoverable from owner
 */

#include <zephyr/kernel.h>
#include <zephyr/spinlock.h>

#include "gale_spinlock_validate.h"

bool z_spin_lock_valid(struct k_spinlock *l)
{
	uintptr_t thread_cpu = l->thread_cpu;

	return gale_spin_lock_valid(thread_cpu,
				    _current_cpu->id) != 0;
}

bool z_spin_unlock_valid(struct k_spinlock *l)
{
	uintptr_t thread_cpu = l->thread_cpu;

	/* Clear the owner — must happen before the validity check,
	 * matching the original C semantics.
	 */
	l->thread_cpu = 0;

	if (thread_cpu == 0) {
		return false;
	}

	return gale_spin_unlock_valid(thread_cpu,
				      _current_cpu->id,
				      (uintptr_t)_current) != 0;
}

void z_spin_lock_set_owner(struct k_spinlock *l)
{
	l->thread_cpu = gale_spin_lock_compute_owner(
		_current_cpu->id, (uintptr_t)_current);
}
