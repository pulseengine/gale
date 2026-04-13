/*
 * Gale PM FFI — verified power management state machine decisions.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#ifndef GALE_PM_H
#define GALE_PM_H

#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

/*
 * Power state codes — match enum pm_state in include/zephyr/pm/state.h.
 */
#define GALE_PM_STATE_ACTIVE           0
#define GALE_PM_STATE_RUNTIME_IDLE     1
#define GALE_PM_STATE_SUSPEND_TO_IDLE  2
#define GALE_PM_STATE_STANDBY          3
#define GALE_PM_STATE_SUSPEND_TO_RAM   4
#define GALE_PM_STATE_SOFT_OFF         5
#define GALE_PM_STATE_COUNT            6

/* ---- Force decision ---- */

struct gale_pm_force_decision {
    uint8_t action;      /* 0=FORCE_OK, 1=TERMINAL */
    uint8_t state;       /* target state (when action=FORCE_OK) */
    uint8_t substate_id;
};

#define GALE_PM_FORCE_OK        0
#define GALE_PM_FORCE_TERMINAL  1

/**
 * Decide whether a PM state force is permissible.
 *
 * PM4: SOFT_OFF is terminal — no force allowed from it.
 * PM5: forced state replaces the pending forced state.
 *
 * @param current_state  Current power state code (0-5).
 * @param target_state   Requested forced state code (0-5).
 * @param substate_id    Requested substate identifier.
 *
 * @return Decision: FORCE_OK or TERMINAL.
 */
struct gale_pm_force_decision gale_pm_force_decide(
    uint8_t current_state,
    uint8_t target_state,
    uint8_t substate_id);

/* ---- Suspend decision ---- */

struct gale_pm_suspend_decision {
    uint8_t action;      /* 0=ENTER_STATE, 1=STAY_ACTIVE */
    uint8_t state;       /* state to enter (when action=ENTER_STATE) */
    uint8_t substate_id;
};

#define GALE_PM_ACTION_ENTER_STATE  0
#define GALE_PM_ACTION_STAY_ACTIVE  1

/**
 * Decide suspend state: forced state wins over policy state.
 *
 * PM5: if a forced state is pending, it takes priority.
 *
 * @param has_forced      Non-zero if a forced state is pending.
 * @param forced_state    Forced state code.
 * @param forced_substate Forced substate id.
 * @param has_policy      Non-zero if policy selected a state.
 * @param policy_state    Policy-selected state code.
 * @param policy_substate Policy substate id.
 *
 * @return Decision: ENTER_STATE or STAY_ACTIVE.
 */
struct gale_pm_suspend_decision gale_pm_suspend_decide(
    uint8_t has_forced,
    uint8_t forced_state,
    uint8_t forced_substate,
    uint8_t has_policy,
    uint8_t policy_state,
    uint8_t policy_substate);

/**
 * Decide whether the residency constraint is satisfied.
 *
 * PM6: only enter a state if there is enough time to justify it.
 * Pass i32::MAX (INT32_MAX) for K_TICKS_FOREVER.
 *
 * @param ticks_available      Ticks until next scheduled event.
 * @param min_residency_ticks  Minimum residency in ticks for the candidate state.
 *
 * @return true if residency is satisfied, false otherwise.
 */
bool gale_pm_residency_ok(int32_t ticks_available, uint32_t min_residency_ticks);

/**
 * Decide whether a power state transition is valid.
 *
 * PM2: ACTIVE can transition to any state.
 * PM3: any non-terminal state can return to ACTIVE.
 * PM4: SOFT_OFF is terminal — no transition out.
 *
 * @param from_state  Source state code (0-5).
 * @param to_state    Target state code (0-5).
 *
 * @return 1 if valid, 0 otherwise.
 */
uint8_t gale_pm_transition_valid(uint8_t from_state, uint8_t to_state);

/* ---- C-side helpers (defined in gale_pm.c) ---- */

/**
 * Checked state force: validates via Rust, updates C-side forced state.
 *
 * @param cpu         CPU index.
 * @param state       Requested pm_state code.
 * @param substate_id Requested substate id.
 *
 * @return 0 on success, -EINVAL on terminal or invalid state.
 */
int gale_pm_state_force_checked(uint8_t cpu, uint8_t state, uint8_t substate_id);

/**
 * Run the suspend decision for the given CPU.
 *
 * Extracts forced state and calls gale_pm_suspend_decide. Callers
 * should then use gale_pm_residency_ok to check if the chosen state
 * fits within the ticks budget before calling pm_state_set().
 *
 * @param cpu          CPU index.
 * @param ticks        Ticks until next scheduled event.
 * @param out_state    Output: chosen state code.
 * @param out_substate Output: chosen substate id.
 *
 * @return 0 if a state was chosen, -EAGAIN if STAY_ACTIVE.
 */
int gale_pm_suspend_checked(
    uint8_t cpu,
    int32_t ticks,
    uint8_t *out_state,
    uint8_t *out_substate);

#ifdef __cplusplus
}
#endif

#endif /* GALE_PM_H */
