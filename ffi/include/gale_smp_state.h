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

/* ---- Phase 2: Full Decision API ---- */

struct gale_smp_start_decision {
    uint8_t action;     /* 0=START_OK, 1=ALL_ACTIVE */
    uint32_t new_active;
};

#define GALE_SMP_ACTION_START_OK    0
#define GALE_SMP_ACTION_ALL_ACTIVE  1

/**
 * Decide whether a CPU can be started.
 *
 * @param active_cpus  Current active CPU count.
 * @param max_cpus     Maximum CPUs in system.
 *
 * @return Decision struct: action + new_active.
 */
struct gale_smp_start_decision gale_smp_start_cpu_decide(
    uint32_t active_cpus, uint32_t max_cpus);

struct gale_smp_stop_decision {
    uint8_t action;     /* 0=STOP_OK, 1=LAST_CPU */
    uint32_t new_active;
};

#define GALE_SMP_ACTION_STOP_OK   0
#define GALE_SMP_ACTION_LAST_CPU  1

/**
 * Decide whether a CPU can be stopped.
 *
 * @param active_cpus  Current active CPU count.
 *
 * @return Decision struct: action + new_active.
 */
struct gale_smp_stop_decision gale_smp_stop_cpu_decide(uint32_t active_cpus);

/* ---- C-side helpers (defined in gale_smp_state.c) ---- */

/**
 * Checked CPU start: validates via Rust decision, updates active count.
 *
 * @param id        CPU id to start.
 * @param max_cpus  Maximum CPUs in system.
 *
 * @return 0 on success, -EBUSY if all CPUs already active.
 */
int gale_smp_cpu_start_checked(int id, unsigned int max_cpus);

/**
 * Checked CPU stop: validates via Rust decision, updates active count.
 *
 * @param id  CPU id to stop.
 *
 * @return 0 on success, -EINVAL if only CPU 0 remains.
 */
int gale_smp_cpu_stop_checked(int id);

/**
 * Get the current Gale-tracked active CPU count.
 *
 * @return Current active CPU count.
 */
unsigned int gale_smp_active_cpus_get(void);

#ifdef __cplusplus
}
#endif

#endif /* GALE_SMP_STATE_H */
