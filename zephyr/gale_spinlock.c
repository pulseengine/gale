/*
 * Copyright (c) 2018 Intel Corporation
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale spinlock — verified nesting-discipline state machine.
 *
 * This shim exposes the high-level ownership/nesting state machine from
 * src/spinlock.rs as C-callable functions.  It complements (does NOT
 * replace) gale_spinlock_validate.c, which handles the low-level
 * thread_cpu encoding.
 *
 * The actual spinlock hardware operations (CAS/ticket lock, IRQ masking,
 * memory barriers, k_spin_lock/k_spin_unlock) remain in Zephyr C.
 * Only the nesting invariants are verified here.
 *
 * Verified properties (Verus proofs):
 *   SL1: lock acquired only when free
 *   SL2: release only by current owner
 *   SL3: nest_count tracks depth correctly
 *   SL4: fully released when nest_count reaches 0
 *   SL5: double-acquire without nesting is rejected
 *
 * Typical usage in a driver or kernel subsystem that uses nested spinlocks:
 *
 *   // Before attempting nested acquire:
 *   if (!gale_spinlock_acquire_check(lock_state.owner)) {
 *       return -EBUSY;
 *   }
 *   // Acquire and update state:
 *   gale_spinlock_acquire_nested(lock_state.owner, lock_state.depth,
 *                                current_tid,
 *                                &lock_state.depth, &lock_state.owner);
 */

#include <zephyr/kernel.h>
#include <zephyr/spinlock.h>

#include "gale_spinlock.h"

/*
 * gale_spinlock_check_and_acquire — convenience wrapper.
 *
 * Atomically checks acquire validity and updates owner/depth fields.
 * Returns 0 on success, -EBUSY if already held.
 *
 * Caller must hold a hardware spinlock before calling this function.
 */
int gale_spinlock_check_and_acquire(uint32_t *owner_tid, uint32_t *nest_count,
				    uint32_t calling_tid)
{
	return gale_spinlock_acquire(*owner_tid, *nest_count, calling_tid,
				     nest_count, owner_tid);
}

/*
 * gale_spinlock_check_and_acquire_nested — nested variant.
 *
 * Same as gale_spinlock_check_and_acquire but supports re-entrancy by
 * the same thread.  Returns 0 on success, -EBUSY otherwise.
 */
int gale_spinlock_check_and_acquire_nested(uint32_t *owner_tid,
					   uint32_t *nest_count,
					   uint32_t calling_tid)
{
	return gale_spinlock_acquire_nested(*owner_tid, *nest_count,
					    calling_tid,
					    nest_count, owner_tid);
}

/*
 * gale_spinlock_check_and_release — convenience wrapper.
 *
 * Validates and applies a spinlock release.
 * Returns 0 on success, -EPERM if calling_tid is not the owner.
 */
int gale_spinlock_check_and_release(uint32_t *owner_tid, uint32_t *nest_count,
				    uint32_t calling_tid)
{
	return gale_spinlock_release(*owner_tid, *nest_count, calling_tid,
				     nest_count, owner_tid);
}
