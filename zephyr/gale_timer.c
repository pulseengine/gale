/*
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale timer — Extract→Decide→Apply pattern.
 *
 * This file provides Gale-validated helper functions for the safety-critical
 * status counter operations in kernel/timer.c.  Unlike the other Gale
 * primitives, this does NOT replace timer.c — the timeout subsystem,
 * scheduling, ISR callbacks, and work queue integration remain in the
 * upstream kernel/timer.c.
 *
 * These helpers are called from kernel/timer.c when CONFIG_GALE_KERNEL_TIMER
 * is enabled:
 *   gale_timer_expiry_handler — Extract→Decide→Apply for status++
 *   gale_timer_status_read   — Extract→Decide→Apply for read+reset
 *
 * Verified operations (Verus proofs):
 *   gale_k_timer_expire_decide — TM5 (increment), TM8 (no overflow)
 *   gale_k_timer_status_decide — TM2 (read + reset to 0)
 *   gale_timer_init_validate   — TM6/TM7 (period classification)
 */

#include <zephyr/kernel.h>
#include <zephyr/sys/check.h>

#include "gale_timer.h"

/**
 * Gale-validated timer expiry: Extract→Decide→Apply.
 *
 * Called from the timer expiry handler in kernel/timer.c in place of
 * the bare `timer->status++`.
 *
 * Extract: read timer->status and timer->period.
 * Decide:  Rust computes new status (saturating increment) and classifies
 *          the timer as periodic or one-shot.
 * Apply:   write new_status back to timer->status.
 *
 * @param timer  Pointer to the timer object.
 *
 * @return 0 on success (status incremented or saturated).
 */
int gale_timer_expiry_handler(struct k_timer *timer)
{
	/* Extract */
	uint32_t status = timer->status;
	uint32_t period = (uint32_t)timer->period.ticks;

	/* Decide */
	struct gale_timer_expire_decision d =
		gale_k_timer_expire_decide(status, period);

	/* Apply */
	timer->status = d.new_status;

	return 0;
}

/**
 * Gale-validated timer status read: Extract→Decide→Apply.
 *
 * Called from k_timer_status_get in kernel/timer.c in place of the
 * bare read-and-reset sequence.
 *
 * Extract: read timer->status.
 * Decide:  Rust returns old status (count) and new status (0).
 * Apply:   write new_status back to timer->status, return count.
 *
 * @param timer  Pointer to the timer object.
 *
 * @return The number of expiry events since the last status read
 *         (or since start/stop).  Timer status is reset to 0.
 */
uint32_t gale_timer_status_read(struct k_timer *timer)
{
	/* Extract */
	uint32_t status = timer->status;

	/* Decide */
	struct gale_timer_status_decision d =
		gale_k_timer_status_decide(status);

	/* Apply */
	timer->status = d.new_status;

	return d.count;
}
