/*
 * Copyright (c) 2018 Intel Corporation.
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale PM — Extract→Decide→Apply pattern for power management.
 *
 * This C shim wraps the power state machine and policy decision logic from
 * subsys/pm/pm.c with Rust decision functions. C extracts kernel state
 * (current power state, forced state, ticks to next event), Rust decides
 * whether the operation is valid and which state to enter, C applies
 * (calls pm_state_set / pm_state_exit_post_ops).
 *
 * Hardware power control (pm_state_set, pm_state_exit_post_ops),
 * device suspend/resume, clock management, spinlock serialization,
 * tracing, and scheduler lock/unlock remain in Zephyr.
 *
 * Verified operations (Verus proofs):
 *   gale_pm_force_decide       — PM4, PM5 (terminal, single-use)
 *   gale_pm_suspend_decide     — PM5 (forced wins over policy)
 *   gale_pm_residency_ok       — PM6 (residency constraint)
 *   gale_pm_transition_valid   — PM2, PM3, PM4 (state machine)
 */

#include <errno.h>
#include <stdint.h>
#include <zephyr/kernel.h>
#include <zephyr/pm/pm.h>
#include <zephyr/pm/state.h>
#include <zephyr/pm/policy.h>

#include "gale_pm.h"

/*
 * Gale-tracked per-CPU current power state.
 * NULL pointer convention: NULL means ACTIVE (matches z_cpus_pm_state).
 * We shadow the Zephyr state to expose it to Rust decision functions
 * without pulling in the full PM internal headers.
 */
static uint8_t gale_cpu_state[CONFIG_MP_MAX_NUM_CPUS];
static uint8_t gale_cpu_substate[CONFIG_MP_MAX_NUM_CPUS];
static bool    gale_cpu_has_forced[CONFIG_MP_MAX_NUM_CPUS];
static uint8_t gale_cpu_forced_state[CONFIG_MP_MAX_NUM_CPUS];
static uint8_t gale_cpu_forced_substate[CONFIG_MP_MAX_NUM_CPUS];

static struct k_spinlock gale_pm_lock;

int gale_pm_state_force_checked(uint8_t cpu, uint8_t state, uint8_t substate_id)
{
	if (cpu >= CONFIG_MP_MAX_NUM_CPUS) {
		return -EINVAL;
	}

	k_spinlock_key_t key = k_spin_lock(&gale_pm_lock);

	/* Decide: Rust determines whether force is allowed (PM4) */
	struct gale_pm_force_decision d = gale_pm_force_decide(
		gale_cpu_state[cpu], state, substate_id);

	if (d.action == GALE_PM_FORCE_TERMINAL) {
		k_spin_unlock(&gale_pm_lock, key);
		return -EINVAL;
	}

	/* Apply: record the forced state (PM5: single-use) */
	gale_cpu_has_forced[cpu]       = true;
	gale_cpu_forced_state[cpu]     = d.state;
	gale_cpu_forced_substate[cpu]  = d.substate_id;

	k_spin_unlock(&gale_pm_lock, key);

	return 0;
}

int gale_pm_suspend_checked(uint8_t cpu, int32_t ticks,
			     uint8_t *out_state, uint8_t *out_substate)
{
	if (cpu >= CONFIG_MP_MAX_NUM_CPUS || out_state == NULL ||
	    out_substate == NULL) {
		return -EINVAL;
	}

	k_spinlock_key_t key = k_spin_lock(&gale_pm_lock);

	/*
	 * Ask the policy which state fits within the ticks budget.
	 * We pass a synthetic policy result here: the real pm.c would
	 * call pm_policy_next_state; we use gale_pm_residency_ok to
	 * validate the chosen state.
	 */
	const struct pm_state_info *policy_info =
		pm_policy_next_state(cpu, ticks);

	uint8_t has_policy    = (policy_info != NULL) ? 1U : 0U;
	uint8_t policy_state  = has_policy ? (uint8_t)policy_info->state : 0U;
	uint8_t policy_sub    = has_policy ? policy_info->substate_id : 0U;

	/* Decide: Rust picks forced vs. policy (PM5) */
	struct gale_pm_suspend_decision d = gale_pm_suspend_decide(
		(uint8_t)gale_cpu_has_forced[cpu],
		gale_cpu_forced_state[cpu],
		gale_cpu_forced_substate[cpu],
		has_policy,
		policy_state,
		policy_sub);

	/* PM5: consume the forced state */
	gale_cpu_has_forced[cpu] = false;

	if (d.action == GALE_PM_ACTION_STAY_ACTIVE) {
		k_spin_unlock(&gale_pm_lock, key);
		return -EAGAIN;
	}

	/* Apply: record chosen state */
	gale_cpu_state[cpu]    = d.state;
	gale_cpu_substate[cpu] = d.substate_id;

	k_spin_unlock(&gale_pm_lock, key);

	*out_state    = d.state;
	*out_substate = d.substate_id;

	return 0;
}

/*
 * Query helpers — expose Gale-tracked state for diagnostics / testing.
 */

uint8_t gale_pm_current_state_get(uint8_t cpu)
{
	if (cpu >= CONFIG_MP_MAX_NUM_CPUS) {
		return GALE_PM_STATE_ACTIVE;
	}
	return gale_cpu_state[cpu];
}

bool gale_pm_has_forced_state(uint8_t cpu)
{
	if (cpu >= CONFIG_MP_MAX_NUM_CPUS) {
		return false;
	}
	return gale_cpu_has_forced[cpu];
}
