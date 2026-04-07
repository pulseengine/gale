/*
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale IPI FFI — verified IPI mask creation for SMP.
 *
 * These functions replace the IPI mask computation from kernel/ipi.c.
 * All other IPI logic (signal_pending_ipi, MetaIRQ, cooperative
 * thread checks) remains native Zephyr C.
 */

#ifndef GALE_IPI_H_
#define GALE_IPI_H_

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Compute the IPI bitmask for a newly ready thread.
 *
 * Iterates over CPUs, checking activity, affinity, and priority
 * to determine which CPUs should receive an IPI.
 *
 * @param current_cpu     CPU executing the scheduling decision.
 * @param target_prio     Priority of the newly ready thread.
 * @param target_cpu_mask CPU affinity mask for the thread.
 * @param cpu_prios       Per-CPU current thread priorities (array of num_cpus).
 * @param cpu_active      Per-CPU active flags (array of num_cpus, 0 or 1).
 * @param num_cpus        Number of CPUs present (arch_num_cpus()).
 * @param max_cpus        CONFIG_MP_MAX_NUM_CPUS (upper bound for bit width).
 *
 * @return Bitmask where bit i is set iff CPU i should receive an IPI.
 *
 * Verified: IP1 (current CPU excluded), IP2 (bounded by num_cpus),
 * IP3 (respects affinity), IP4 (priority comparison), IP5 (bounded by max_cpus).
 */
uint32_t gale_compute_ipi_mask(uint32_t current_cpu,
                               int32_t target_prio,
                               uint32_t target_cpu_mask,
                               const int32_t *cpu_prios,
                               const uint8_t *cpu_active,
                               uint32_t num_cpus,
                               uint32_t max_cpus);

/**
 * Validate a previously computed IPI mask.
 *
 * Checks structural properties that must hold for any valid mask:
 * - current CPU bit is not set
 * - no bits at or above max_cpus are set
 *
 * @param mask        The IPI mask to validate.
 * @param current_cpu The CPU that computed the mask.
 * @param max_cpus    CONFIG_MP_MAX_NUM_CPUS.
 *
 * @return 1 if valid, 0 if invalid.
 *
 * Verified: IP1 (current CPU exclusion), IP5 (bounded by max_cpus).
 */
int32_t gale_validate_ipi_mask(uint32_t mask,
                               uint32_t current_cpu,
                               uint32_t max_cpus);

/*
 * C-shim wrapper — declared here so callers (e.g. gale_sched.c) can use
 * the verified mask creation without pulling in <ipi.h> internals.
 *
 * Only available when CONFIG_SMP && !CONFIG_IPI_OPTIMIZE; the definition
 * lives in zephyr/gale_ipi.c.
 */
#if defined(CONFIG_SMP) && !defined(CONFIG_IPI_OPTIMIZE)
struct k_thread;   /* forward declaration — avoid pulling in kernel.h */
atomic_val_t gale_ipi_mask_create(struct k_thread *thread);
#endif /* CONFIG_SMP && !CONFIG_IPI_OPTIMIZE */

#ifdef __cplusplus
}
#endif

#endif /* GALE_IPI_H_ */
