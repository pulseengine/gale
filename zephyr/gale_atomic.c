/*
 * Copyright (c) 2016 Wind River Systems, Inc.
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale atomic — Extract→Decide→Apply pattern for kernel/atomic_c.c.
 *
 * This replaces kernel/atomic_c.c.  C handles the spinlock-based
 * atomicity (k_spin_lock/k_spin_unlock, IRQ masking); Rust decides
 * the arithmetic result under a pure function call.
 *
 * Pattern for each read-modify-write operation:
 *   Extract: read *target under spinlock
 *   Decide:  call gale_atomic_*(current, ...) — returns {old_val, new_val}
 *   Apply:   write new_val to *target; unlock; return old_val
 *
 * Verified operations (Verus proofs):
 *   AT1: add — returns old, stores wrapping add
 *   AT2: sub — returns old, stores wrapping sub
 *   AT3: cas — succeeds only when current == expected
 *   AT4: cas — failure leaves value unchanged
 *   AT6: wrapping semantics for add/sub
 *
 * NOTE: Zephyr's atomic_t is `long` on most platforms.  This shim
 * models the value as u32 (matching Cortex-M atomic width).  On
 * 64-bit targets the cast to/from uint32_t truncates; this matches
 * the Verus model which is also u32-scoped.
 */

#include <zephyr/kernel.h>
#include <zephyr/arch/cpu.h>
#include <zephyr/spinlock.h>

#include "gale_atomic.h"

/* Per Zephyr's atomic_c.c: one global spinlock guards all atomic_t accesses
 * in the software-emulation path (CONFIG_ATOMIC_OPERATIONS_C).
 */
static struct k_spinlock atomic_lock;

/* ---------------------------------------------------------------------------
 * atomic_get (not under spinlock — read is naturally atomic on 32-bit)
 * ---------------------------------------------------------------------------
 */

atomic_val_t z_impl_atomic_get(const atomic_t *target)
{
	/* Extract */
	uint32_t current = (uint32_t)*target;

	/* Decide (pure: identity) */
	uint32_t result = gale_atomic_get(current);

	return (atomic_val_t)result;
}

#ifdef CONFIG_USERSPACE
static inline atomic_val_t z_vrfy_atomic_get(const atomic_t *target)
{
	K_OOPS(K_SYSCALL_MEMORY_READ(target, sizeof(*target)));
	return z_impl_atomic_get(target);
}
#include <zephyr/syscalls/atomic_get_mrsh.c>
#endif /* CONFIG_USERSPACE */

/* ---------------------------------------------------------------------------
 * atomic_set
 * ---------------------------------------------------------------------------
 */

atomic_val_t z_impl_atomic_set(atomic_t *target, atomic_val_t value)
{
	k_spinlock_key_t key = k_spin_lock(&atomic_lock);

	/* Extract */
	uint32_t current = (uint32_t)*target;

	/* Decide */
	struct gale_atomic_rmw_decision d =
		gale_atomic_set(current, (uint32_t)value);

	/* Apply */
	*target = (atomic_t)d.new_val;

	k_spin_unlock(&atomic_lock, key);

	return (atomic_val_t)d.old_val;
}

#ifdef CONFIG_USERSPACE
static inline atomic_val_t z_vrfy_atomic_set(atomic_t *target,
					      atomic_val_t value)
{
	K_OOPS(K_SYSCALL_MEMORY_WRITE(target, sizeof(*target)));
	return z_impl_atomic_set(target, value);
}
#include <zephyr/syscalls/atomic_set_mrsh.c>
#endif /* CONFIG_USERSPACE */

/* ---------------------------------------------------------------------------
 * atomic_add
 * ---------------------------------------------------------------------------
 */

atomic_val_t z_impl_atomic_add(atomic_t *target, atomic_val_t value)
{
	k_spinlock_key_t key = k_spin_lock(&atomic_lock);

	uint32_t current = (uint32_t)*target;

	/* Decide: AT1 + AT6 (wrapping add, returns old) */
	struct gale_atomic_rmw_decision d =
		gale_atomic_add(current, (uint32_t)value);

	*target = (atomic_t)d.new_val;

	k_spin_unlock(&atomic_lock, key);

	return (atomic_val_t)d.old_val;
}

#ifdef CONFIG_USERSPACE
static inline atomic_val_t z_vrfy_atomic_add(atomic_t *target,
					      atomic_val_t value)
{
	K_OOPS(K_SYSCALL_MEMORY_WRITE(target, sizeof(*target)));
	return z_impl_atomic_add(target, value);
}
#include <zephyr/syscalls/atomic_add_mrsh.c>
#endif /* CONFIG_USERSPACE */

/* ---------------------------------------------------------------------------
 * atomic_sub
 * ---------------------------------------------------------------------------
 */

atomic_val_t z_impl_atomic_sub(atomic_t *target, atomic_val_t value)
{
	k_spinlock_key_t key = k_spin_lock(&atomic_lock);

	uint32_t current = (uint32_t)*target;

	/* Decide: AT2 + AT6 (wrapping sub, returns old) */
	struct gale_atomic_rmw_decision d =
		gale_atomic_sub(current, (uint32_t)value);

	*target = (atomic_t)d.new_val;

	k_spin_unlock(&atomic_lock, key);

	return (atomic_val_t)d.old_val;
}

#ifdef CONFIG_USERSPACE
static inline atomic_val_t z_vrfy_atomic_sub(atomic_t *target,
					      atomic_val_t value)
{
	K_OOPS(K_SYSCALL_MEMORY_WRITE(target, sizeof(*target)));
	return z_impl_atomic_sub(target, value);
}
#include <zephyr/syscalls/atomic_sub_mrsh.c>
#endif /* CONFIG_USERSPACE */

/* ---------------------------------------------------------------------------
 * atomic_or
 * ---------------------------------------------------------------------------
 */

atomic_val_t z_impl_atomic_or(atomic_t *target, atomic_val_t value)
{
	k_spinlock_key_t key = k_spin_lock(&atomic_lock);

	uint32_t current = (uint32_t)*target;
	struct gale_atomic_rmw_decision d =
		gale_atomic_or(current, (uint32_t)value);

	*target = (atomic_t)d.new_val;
	k_spin_unlock(&atomic_lock, key);

	return (atomic_val_t)d.old_val;
}

#ifdef CONFIG_USERSPACE
static inline atomic_val_t z_vrfy_atomic_or(atomic_t *target,
					     atomic_val_t value)
{
	K_OOPS(K_SYSCALL_MEMORY_WRITE(target, sizeof(*target)));
	return z_impl_atomic_or(target, value);
}
#include <zephyr/syscalls/atomic_or_mrsh.c>
#endif /* CONFIG_USERSPACE */

/* ---------------------------------------------------------------------------
 * atomic_and
 * ---------------------------------------------------------------------------
 */

atomic_val_t z_impl_atomic_and(atomic_t *target, atomic_val_t value)
{
	k_spinlock_key_t key = k_spin_lock(&atomic_lock);

	uint32_t current = (uint32_t)*target;
	struct gale_atomic_rmw_decision d =
		gale_atomic_and(current, (uint32_t)value);

	*target = (atomic_t)d.new_val;
	k_spin_unlock(&atomic_lock, key);

	return (atomic_val_t)d.old_val;
}

#ifdef CONFIG_USERSPACE
static inline atomic_val_t z_vrfy_atomic_and(atomic_t *target,
					      atomic_val_t value)
{
	K_OOPS(K_SYSCALL_MEMORY_WRITE(target, sizeof(*target)));
	return z_impl_atomic_and(target, value);
}
#include <zephyr/syscalls/atomic_and_mrsh.c>
#endif /* CONFIG_USERSPACE */

/* ---------------------------------------------------------------------------
 * atomic_xor
 * ---------------------------------------------------------------------------
 */

atomic_val_t z_impl_atomic_xor(atomic_t *target, atomic_val_t value)
{
	k_spinlock_key_t key = k_spin_lock(&atomic_lock);

	uint32_t current = (uint32_t)*target;
	struct gale_atomic_rmw_decision d =
		gale_atomic_xor(current, (uint32_t)value);

	*target = (atomic_t)d.new_val;
	k_spin_unlock(&atomic_lock, key);

	return (atomic_val_t)d.old_val;
}

#ifdef CONFIG_USERSPACE
static inline atomic_val_t z_vrfy_atomic_xor(atomic_t *target,
					      atomic_val_t value)
{
	K_OOPS(K_SYSCALL_MEMORY_WRITE(target, sizeof(*target)));
	return z_impl_atomic_xor(target, value);
}
#include <zephyr/syscalls/atomic_xor_mrsh.c>
#endif /* CONFIG_USERSPACE */

/* ---------------------------------------------------------------------------
 * atomic_nand
 * ---------------------------------------------------------------------------
 */

atomic_val_t z_impl_atomic_nand(atomic_t *target, atomic_val_t value)
{
	k_spinlock_key_t key = k_spin_lock(&atomic_lock);

	uint32_t current = (uint32_t)*target;
	struct gale_atomic_rmw_decision d =
		gale_atomic_nand(current, (uint32_t)value);

	*target = (atomic_t)d.new_val;
	k_spin_unlock(&atomic_lock, key);

	return (atomic_val_t)d.old_val;
}

#ifdef CONFIG_USERSPACE
static inline atomic_val_t z_vrfy_atomic_nand(atomic_t *target,
					       atomic_val_t value)
{
	K_OOPS(K_SYSCALL_MEMORY_WRITE(target, sizeof(*target)));
	return z_impl_atomic_nand(target, value);
}
#include <zephyr/syscalls/atomic_nand_mrsh.c>
#endif /* CONFIG_USERSPACE */

/* ---------------------------------------------------------------------------
 * atomic_cas
 * ---------------------------------------------------------------------------
 */

bool z_impl_atomic_cas(atomic_t *target, atomic_val_t old_value,
		       atomic_val_t new_value)
{
	k_spinlock_key_t key = k_spin_lock(&atomic_lock);

	/* Extract */
	uint32_t current = (uint32_t)*target;

	/* Decide: AT3 (success iff current == expected),
	 *         AT4 (failure -> no write)
	 */
	struct gale_atomic_cas_decision d =
		gale_atomic_cas(current, (uint32_t)old_value, (uint32_t)new_value);

	/* Apply */
	if (d.success) {
		*target = (atomic_t)d.new_val;
	}

	k_spin_unlock(&atomic_lock, key);

	return (bool)d.success;
}

#ifdef CONFIG_USERSPACE
static inline bool z_vrfy_atomic_cas(atomic_t *target, atomic_val_t old_value,
				      atomic_val_t new_value)
{
	K_OOPS(K_SYSCALL_MEMORY_WRITE(target, sizeof(*target)));
	return z_impl_atomic_cas(target, old_value, new_value);
}
#include <zephyr/syscalls/atomic_cas_mrsh.c>
#endif /* CONFIG_USERSPACE */

/* ---------------------------------------------------------------------------
 * atomic_ptr_cas — pointer variant (same logic, uintptr_t cast)
 * ---------------------------------------------------------------------------
 */

bool z_impl_atomic_ptr_cas(atomic_ptr_t *target, atomic_ptr_val_t old_value,
			    atomic_ptr_val_t new_value)
{
	k_spinlock_key_t key = k_spin_lock(&atomic_lock);

	uint32_t current = (uint32_t)(uintptr_t)*target;
	struct gale_atomic_cas_decision d =
		gale_atomic_cas(current,
				(uint32_t)(uintptr_t)old_value,
				(uint32_t)(uintptr_t)new_value);

	if (d.success) {
		*target = (atomic_ptr_val_t)(uintptr_t)d.new_val;
	}

	k_spin_unlock(&atomic_lock, key);

	return (bool)d.success;
}

#ifdef CONFIG_USERSPACE
static inline bool z_vrfy_atomic_ptr_cas(atomic_ptr_t *target,
					  atomic_ptr_val_t old_value,
					  atomic_ptr_val_t new_value)
{
	K_OOPS(K_SYSCALL_MEMORY_WRITE(target, sizeof(*target)));
	return z_impl_atomic_ptr_cas(target, old_value, new_value);
}
#include <zephyr/syscalls/atomic_ptr_cas_mrsh.c>
#endif /* CONFIG_USERSPACE */
