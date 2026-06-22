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
#include <kernel_internal.h>

#include "gale_spinlock_validate.h"

/*
 * U-9 / CC-9: mirror the Verus `requires cpu_id_valid(cpu)` precondition
 * (cpu < MAX_CPUS = 4) at the C boundary. The owner encoding OR's the CPU
 * ID into the low 2 bits of an aligned thread pointer; with more than 4
 * CPUs those bits overflow into the thread pointer field and `& CPU_MASK`
 * silently collapses distinct CPUs onto the same tag. A same-CPU reacquire
 * on cpu_id >= 4 would then be reported "valid" and deadlock on release.
 *
 * If CONFIG_MP_MAX_NUM_CPUS is raised beyond 4, widen MAX_CPUS / CPU_MASK
 * in src/spinlock_validate.rs, regenerate the plain/ mirror, and update
 * this assertion together — they are a single ABI contract.
 */
BUILD_ASSERT(CONFIG_MP_MAX_NUM_CPUS <= 4,
	     "gale_spinlock_validate assumes CPU_MASK fits 2 bits "
	     "(MAX_CPUS = 4); update src/spinlock_validate.rs CPU_MASK "
	     "and this assertion together.");

bool z_spin_lock_valid(struct k_spinlock *l)
{
	uintptr_t thread_cpu = l->thread_cpu;

	/* Early-boot arm, symmetric with z_spin_unlock_valid's zero-tag arm
	 * (#58, gale#98). Before a CPU's dummy thread exists, _current == NULL,
	 * so set_owner encoded the tag as (cpu_id | NULL) == cpu_id — a non-zero
	 * tag on an AP (CPU id != 0). A re-acquire on that same still-NULL CPU
	 * would then match (thread_cpu & CPU_MASK) == _current_cpu->id and be
	 * reported as a same-CPU deadlock, which recurses assert->printk->
	 * spinlock pre-console and hangs boot silently (the AP-bringup race that
	 * #58 left unpatched on this arm; see gale#98). Real deadlock detection
	 * needs a real owning thread, so skip the check while _current == NULL —
	 * stock spinlock_validate.c tolerates the boot tag the same way. Inert
	 * post-boot: once threads exist _current != NULL and the full Verus-backed
	 * check below runs, protected by its thread_ptr_valid precondition.
	 */
	if (_current == NULL) {
		return true;
	}

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

	/* Early-boot arm, symmetric with z_spin_lock_valid and the zero-tag arm
	 * below (#58, gale#98). On an AP before its dummy thread exists,
	 * _current == NULL and the tag is (cpu_id | NULL) == cpu_id != 0, so the
	 * zero-tag arm below does NOT catch it and we would call the FFI with a
	 * NULL thread — violating gale_spin_unlock_valid's thread_ptr_valid
	 * (non-NULL) precondition (erased at the C boundary). Validation needs a
	 * real owning thread, so skip it while _current == NULL (the owner is
	 * already cleared above). Must precede the _current deref below. Inert
	 * post-boot: _current != NULL, so the full check runs.
	 */
	if (_current == NULL) {
		return true;
	}

	/* Edge case: an ISR aborted _current, leaving it as a dummy
	 * thread.  The spinlock was locked by the pre-abort thread,
	 * so the owner check below would fail.  Skip validation in
	 * this case, matching upstream spinlock_validate.c:29-32.
	 */
	if (arch_is_in_isr() && _current->base.thread_state & _THREAD_DUMMY) {
		return true;
	}

	if (thread_cpu == 0) {
		/* A zero owner tag is legitimate in exactly one window: early
		 * boot before the dummy thread exists, where set_owner encoded
		 * (cpu 0 | _current == NULL) == 0. Stock spinlock_validate.c
		 * accepts this via its plain comparison (0 == (0 | NULL)) —
		 * e.g. x86_64 virt_region_init() locks the virt-region bitmap
		 * long before the console or threads come up; rejecting it
		 * here recursed assert->printk->spinlock and hung boot
		 * silently (no console yet). Mirror stock for this arm; the
		 * Verus call below stays protected by its thread_ptr_valid
		 * (non-NULL, aligned) precondition. Post-boot, a zero tag
		 * still means unlock-of-unheld and is rejected, because
		 * (_current | cpu_id) != 0 once threads exist.
		 */
		return ((uintptr_t)_current | _current_cpu->id) == 0;
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
