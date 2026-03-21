/*
 * Copyright (c) 2022 Intel corporation
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale SMP state — phase 2: Extract→Decide→Apply pattern.
 *
 * This C shim wraps the CPU state tracking from kernel/smp.c with
 * Rust decision structs.  C extracts kernel state (active CPU count,
 * max CPUs), Rust decides whether the operation is valid, C applies.
 *
 * IPI signaling, interrupt stack setup, and arch CPU start remain
 * in Zephyr.
 *
 * Verified operations (Verus proofs):
 *   gale_smp_start_cpu_decide — SM2 (start, active += 1)
 *   gale_smp_stop_cpu_decide  — SM3 (stop, CPU 0 never stops)
 */

#include <zephyr/kernel.h>
#include <zephyr/kernel_structs.h>
#include <zephyr/kernel/smp.h>
#include <zephyr/spinlock.h>

#include "gale_smp_state.h"

/* Gale active CPU count — tracks how many CPUs are running */
static unsigned int gale_active_cpus = 1; /* CPU 0 always starts active */

static struct k_spinlock gale_smp_lock;

int gale_smp_cpu_start_checked(int id, unsigned int max_cpus)
{
	k_spinlock_key_t key = k_spin_lock(&gale_smp_lock);

	/* Decide: Rust determines whether we can start another CPU */
	struct gale_smp_start_decision d = gale_smp_start_cpu_decide(
		gale_active_cpus, (uint32_t)max_cpus);

	if (d.action == GALE_SMP_ACTION_ALL_ACTIVE) {
		k_spin_unlock(&gale_smp_lock, key);
		return -EBUSY;
	}

	/* Apply: update active count from Rust's decision */
	gale_active_cpus = d.new_active;

	k_spin_unlock(&gale_smp_lock, key);

	return 0;
}

int gale_smp_cpu_stop_checked(int id)
{
	k_spinlock_key_t key = k_spin_lock(&gale_smp_lock);

	/* Decide: Rust determines whether stopping is valid */
	struct gale_smp_stop_decision d = gale_smp_stop_cpu_decide(
		gale_active_cpus);

	if (d.action == GALE_SMP_ACTION_LAST_CPU) {
		k_spin_unlock(&gale_smp_lock, key);
		return -EINVAL;
	}

	/* Apply: update active count from Rust's decision */
	gale_active_cpus = d.new_active;

	k_spin_unlock(&gale_smp_lock, key);

	return 0;
}

unsigned int gale_smp_active_cpus_get(void)
{
	return gale_active_cpus;
}
