/*
 * Gale SMP State FFI — verified SMP CPU state tracking.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#ifndef GALE_SMP_STATE_H
#define GALE_SMP_STATE_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Validate starting a CPU: increment active_cpus.
 *
 * @param active_cpus  Current active CPU count.
 * @param max_cpus     Maximum CPUs in system.
 * @param new_active   Output: active_cpus + 1.
 *
 * @return 0 on success, -EBUSY if all active, -EINVAL on null pointer.
 */
int32_t gale_smp_start_cpu_validate(uint32_t active_cpus,
                                     uint32_t max_cpus,
                                     uint32_t *new_active);

/**
 * Validate stopping a CPU: decrement active_cpus (min 1).
 *
 * @param active_cpus  Current active CPU count.
 * @param new_active   Output: active_cpus - 1.
 *
 * @return 0 on success, -EINVAL if only CPU 0 remains or null pointer.
 */
int32_t gale_smp_stop_cpu_validate(uint32_t active_cpus,
                                    uint32_t *new_active);

#ifdef __cplusplus
}
#endif

#endif /* GALE_SMP_STATE_H */
