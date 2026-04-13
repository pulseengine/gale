/*
 * Gale Usage FFI — verified thread runtime statistics decision logic.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#ifndef GALE_USAGE_H
#define GALE_USAGE_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/*
 * Action codes returned by decision functions.
 */

/** sys enable/disable: already in target state — do nothing. */
#define GALE_USAGE_SYS_NOOP    0
/** sys enable/disable: apply state change to all CPUs. */
#define GALE_USAGE_SYS_APPLY   1

/** start_decide: record usage0 only (no window tracking). */
#define GALE_USAGE_START_RECORD_ONLY    0
/** start_decide: record usage0 and update window counter. */
#define GALE_USAGE_START_RECORD_WINDOW  1

/** stop_decide: usage0 == 0 — skip accumulation. */
#define GALE_USAGE_STOP_SKIP       0
/** stop_decide: usage0 != 0 — compute and accumulate cycles. */
#define GALE_USAGE_STOP_ACCUMULATE 1

/**
 * Decide whether k_sys_runtime_stats_enable() should apply changes.
 *
 * Maps usage.c:283-293: if current_cpu->usage->track_usage is already
 * true, there is nothing to do.
 *
 * @param current_tracking  Non-zero if the current CPU is already tracking.
 *
 * @return GALE_USAGE_SYS_NOOP if already enabled,
 *         GALE_USAGE_SYS_APPLY otherwise.
 *
 * Verified: US4 (idempotent enable).
 */
uint8_t gale_usage_sys_enable_decide(uint32_t current_tracking);

/**
 * Decide whether k_sys_runtime_stats_disable() should apply changes.
 *
 * Maps usage.c:317-326: if current_cpu->usage->track_usage is already
 * false, there is nothing to do.
 *
 * @param current_tracking  Non-zero if the current CPU is currently tracking.
 *
 * @return GALE_USAGE_SYS_NOOP if already disabled,
 *         GALE_USAGE_SYS_APPLY otherwise.
 *
 * Verified: US4 (idempotent disable).
 */
uint8_t gale_usage_sys_disable_decide(uint32_t current_tracking);

/**
 * Decide what z_sched_usage_start() should do for this thread.
 *
 * Maps usage.c:74-97: if track_usage is true (analysis mode enabled),
 * the C shim should also reset the current window and increment num_windows.
 *
 * @param track_usage  Non-zero if this thread has usage analysis enabled.
 *
 * @return GALE_USAGE_START_RECORD_ONLY   — set cpu->usage0 = now only.
 *         GALE_USAGE_START_RECORD_WINDOW — set usage0 and update window.
 *
 * Verified: US1 (window tracking only when track_usage set).
 */
uint8_t gale_usage_start_decide(uint32_t track_usage);

/**
 * Decide what z_sched_usage_stop() should do.
 *
 * Maps usage.c:107: `if (u0 != 0)` — only accumulate if start was
 * recorded; a zero usage0 means start was not called or stop already ran.
 *
 * @param usage0  Current value of cpu->usage0 (snapshot from start).
 *
 * @return GALE_USAGE_STOP_SKIP       — usage0 == 0; do nothing.
 *         GALE_USAGE_STOP_ACCUMULATE — compute and accumulate cycles.
 *
 * Verified: US2 (accumulate only when usage0 != 0).
 */
uint8_t gale_usage_stop_decide(uint32_t usage0);

/**
 * Compute average cycles, guarding against division by zero.
 *
 * Maps usage.c:155-159, 211-215: the `if (num_windows == 0)` guard.
 *
 * @param total_cycles  Accumulated total execution cycles.
 * @param num_windows   Number of scheduling windows.
 * @param out_average   Output: computed average (set to 0 on overflow guard).
 *
 * @return 0 on success, -EINVAL if out_average is NULL.
 *
 * Verified: US5 (no division by zero).
 */
int32_t gale_usage_average_cycles(uint64_t total_cycles,
                                   uint32_t num_windows,
                                   uint64_t *out_average);

/**
 * Compute elapsed cycles using wrapping u32 subtraction.
 *
 * Maps usage.c:108: `uint32_t cycles = usage_now() - u0;`
 * Uses wrapping subtraction to correctly handle u32 counter wrap-around.
 *
 * @param now     Current timestamp from usage_now().
 * @param usage0  Snapshot recorded at z_sched_usage_start().
 *
 * @return Elapsed cycles (wrapping arithmetic).
 *
 * Verified: US2 (elapsed used by stop accumulation path).
 */
uint32_t gale_usage_elapsed_cycles(uint32_t now, uint32_t usage0);

/**
 * Accumulate cycles into a thread's total, checking for overflow.
 *
 * Called by the C shim's stop/disable path after computing elapsed cycles.
 *
 * @param total_cycles  Pointer to the thread's accumulated cycle counter.
 * @param cycles        Cycles to add (from gale_usage_elapsed_cycles).
 *
 * @return  0 (OK)         — *total_cycles updated.
 *         -EOVERFLOW      — would overflow u64; *total_cycles unchanged.
 *         -EINVAL         — null pointer.
 *
 * Verified: US6 (total_cycles monotonically non-decreasing).
 */
int32_t gale_usage_accumulate(uint64_t *total_cycles, uint32_t cycles);

#ifdef __cplusplus
}
#endif

#endif /* GALE_USAGE_H */
