/*
 * Copyright (c) 2024 Intel Corporation
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale CPU mask — formally verified CPU affinity mask arithmetic.
 *
 * This C shim replaces cpu_mask_mod() from kernel/cpu_mask.c with the
 * Gale formally verified Rust implementation.  C handles spinlocking,
 * thread state queries, polling, and userspace syscalls.  Rust handles
 * the mask arithmetic (enable/disable/pin-only validation).
 *
 * Verified operations (Verus + Rocq proofs):
 *   gale_cpu_mask_mod      — CM1-CM5 (running guard, pin-only, formula,
 *                            nonzero, overflow)
 *   gale_validate_pin_mask — CM2 (power-of-two)
 *   gale_cpu_pin_compute   — CM6 (bounds check)
 */

#include <zephyr/kernel.h>
#include <zephyr/kernel_structs.h>
#include <zephyr/spinlock.h>
#include <ksched.h>

#include "gale_cpu_mask.h"

#ifdef CONFIG_SCHED_CPU_MASK

/* Use the scheduler's global spinlock, defined in sched.c (or gale_sched.c). */
extern struct k_spinlock _sched_spinlock;

static int gale_cpu_mask_mod_wrapper(struct k_thread *thread,
				     uint32_t enable, uint32_t disable,
				     uint32_t pin_only)
{
	k_spinlock_key_t key = k_spin_lock(&_sched_spinlock);

	uint32_t is_running = !z_is_thread_prevented_from_running(thread) ? 1U : 0U;

	struct gale_cpu_mask_result r = gale_cpu_mask_mod(
		thread->base.cpu_mask, enable, disable, is_running, pin_only);

	if (r.err == 0) {
		thread->base.cpu_mask = r.mask;
	}

	k_spin_unlock(&_sched_spinlock, key);

	return (int)r.err;
}

/*
 * Public API — these are plain (non-syscall) functions matching the
 * declarations in <zephyr/kernel.h> under CONFIG_SCHED_CPU_MASK.
 * They replace the equivalents from kernel/cpu_mask.c (excluded by the
 * fork-guard in kernel/CMakeLists.txt when CONFIG_GALE_KERNEL_CPU_MASK=y).
 */
int k_thread_cpu_mask_clear(k_tid_t thread)
{
	return gale_cpu_mask_mod_wrapper(thread, 0U, 0xFFFFFFFFU, 0U);
}

int k_thread_cpu_mask_enable_all(k_tid_t thread)
{
	return gale_cpu_mask_mod_wrapper(thread, 0xFFFFFFFFU, 0U, 0U);
}

int k_thread_cpu_mask_enable(k_tid_t thread, int cpu)
{
	return gale_cpu_mask_mod_wrapper(thread, BIT(cpu), 0U, 0U);
}

int k_thread_cpu_mask_disable(k_tid_t thread, int cpu)
{
	return gale_cpu_mask_mod_wrapper(thread, 0U, BIT(cpu), 0U);
}

int k_thread_cpu_pin(k_tid_t thread, int cpu)
{
#ifdef CONFIG_SCHED_CPU_MASK_PIN_ONLY
	struct gale_cpu_mask_result pin = gale_cpu_pin_compute(
		(uint32_t)cpu, (uint32_t)CONFIG_MP_MAX_NUM_CPUS);

	if (pin.err != 0) {
		return (int)pin.err;
	}

	return gale_cpu_mask_mod_wrapper(thread, pin.mask, ~pin.mask, 1U);
#else
	uint32_t mask = BIT(cpu);

	return gale_cpu_mask_mod_wrapper(thread, mask, ~mask, 0U);
#endif /* CONFIG_SCHED_CPU_MASK_PIN_ONLY */
}

#endif /* CONFIG_SCHED_CPU_MASK */
