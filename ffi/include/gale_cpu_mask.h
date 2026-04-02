/*
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale CPU mask FFI — verified CPU affinity mask arithmetic.
 *
 * These functions replace the mask arithmetic from kernel/cpu_mask.c.
 * Locking, thread state queries, polling, and userspace syscalls
 * remain native Zephyr C.
 *
 * Verified: CM1-CM6 (running guard, pin-only, formula, nonzero,
 * overflow, bounds).
 */

#ifndef GALE_CPU_MASK_H_
#define GALE_CPU_MASK_H_

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Result of a CPU mask modification or pin computation.
 *
 * On success (err == 0): mask holds the new CPU affinity mask.
 * On failure (err != 0): mask is 0 or unchanged (operation-dependent).
 */
struct gale_cpu_mask_result {
    uint32_t mask;
    int32_t  err;
};

/**
 * Core CPU mask modification — decide new mask from enable/disable bits.
 *
 * new_mask = (current_mask | enable) & ~disable
 *
 * Rejects: running threads (is_running != 0), zero result masks,
 * and non-power-of-two results when pin_only != 0.
 *
 * @param current_mask  Thread's current CPU affinity mask.
 * @param enable        Bits to OR into the mask.
 * @param disable       Bits to AND-complement out of the mask.
 * @param is_running    Nonzero if the thread is currently running.
 * @param pin_only      Nonzero if CONFIG_SCHED_CPU_MASK_PIN_ONLY.
 *
 * @return Result with new mask and error code (0 or -EINVAL).
 *
 * Verified: CM1-CM5.
 */
struct gale_cpu_mask_result gale_cpu_mask_mod(
    uint32_t current_mask, uint32_t enable, uint32_t disable,
    uint32_t is_running, uint32_t pin_only);

/**
 * Validate whether a mask is a valid PIN_ONLY mask (exactly one bit set).
 *
 * @param mask  The mask to validate.
 *
 * @return 1 if valid (power of two, nonzero), 0 otherwise.
 *
 * Verified: CM2.
 */
int32_t gale_validate_pin_mask(uint32_t mask);

/**
 * Compute the pin mask for a specific CPU: BIT(cpu_id).
 *
 * @param cpu_id    CPU index (must be < max_cpus).
 * @param max_cpus  Maximum CPUs in system (must be <= 32).
 *
 * @return Result with single-bit mask and error code (0 or -EINVAL).
 *
 * Verified: CM6.
 */
struct gale_cpu_mask_result gale_cpu_pin_compute(
    uint32_t cpu_id, uint32_t max_cpus);

#ifdef __cplusplus
}
#endif

#endif /* GALE_CPU_MASK_H_ */
