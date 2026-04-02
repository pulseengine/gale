/*
 * Copyright (c) 2022 Intel Corporation
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale IPI — formally verified IPI mask creation.
 *
 * This C shim replaces ipi_mask_create() from kernel/ipi.c with the
 * Gale formally verified Rust implementation. C extracts per-CPU
 * thread priorities and active states, Rust computes the mask.
 *
 * IPI signaling (signal_pending_ipi), MetaIRQ preemption override,
 * cooperative thread guard, and CONFIG_IPI_OPTIMIZE bypass remain
 * native Zephyr C.
 *
 * Verified operations (Verus proofs):
 *   gale_compute_ipi_mask  — IP1-IP5 (mask correctness)
 *   gale_validate_ipi_mask — IP1, IP5 (structural validation)
 */

#include <zephyr/kernel.h>
#include <zephyr/kernel_structs.h>
#include <zephyr/spinlock.h>
#include <ksched.h>
#include <ipi.h>

#include "gale_ipi.h"

/**
 * Verified replacement for ipi_mask_create() from kernel/ipi.c:29-70.
 *
 * Extracts per-CPU thread priorities and active states from Zephyr's
 * _kernel.cpus[] array, then delegates mask computation to verified
 * Rust code.
 *
 * @param thread  The newly ready thread that may need IPIs sent.
 *
 * @return Bitmask of CPUs that should receive an IPI.
 */
#if defined(CONFIG_SMP) && !defined(CONFIG_IPI_OPTIMIZE)
atomic_val_t gale_ipi_mask_create(struct k_thread *thread)
{
	unsigned int num_cpus = (unsigned int)arch_num_cpus();
	uint32_t max_cpus = CONFIG_MP_MAX_NUM_CPUS;
	uint32_t current_cpu = _current_cpu->id;

	/* Extract per-CPU data into stack arrays (max 16 CPUs) */
	int32_t cpu_prios[16];
	uint8_t cpu_active[16];

	for (unsigned int i = 0; i < num_cpus && i < 16; i++) {
		struct k_thread *cpu_thread = _kernel.cpus[i].current;

		if (cpu_thread != NULL) {
			cpu_prios[i] = cpu_thread->base.prio;
			cpu_active[i] = _kernel.cpus[i].active ? 1 : 0;
		} else {
			cpu_prios[i] = -1;  /* idle priority */
			cpu_active[i] = 0;
		}
	}

	int32_t target_prio = thread->base.prio;
	uint32_t target_cpu_mask;

#ifdef CONFIG_SCHED_CPU_MASK
	target_cpu_mask = thread->base.cpu_mask;
#else
	/* No CPU mask support — all CPUs eligible */
	target_cpu_mask = (num_cpus < 32) ? ((1U << num_cpus) - 1U) : 0xFFFFFFFFU;
#endif

	return (atomic_val_t)gale_compute_ipi_mask(
		current_cpu, target_prio, target_cpu_mask,
		cpu_prios, cpu_active, num_cpus, max_cpus);
}
#endif /* CONFIG_SMP && !CONFIG_IPI_OPTIMIZE */
