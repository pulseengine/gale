/*
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale timer — verified status counter arithmetic.
 *
 * This file provides Gale-validated helper functions for the safety-critical
 * status counter operations in kernel/timer.c.  Unlike the other Gale
 * primitives, this does NOT replace timer.c — the timeout subsystem,
 * scheduling, ISR callbacks, and work queue integration remain in the
 * upstream kernel/timer.c.
 *
 * These helpers are called from kernel/timer.c when CONFIG_GALE_KERNEL_TIMER
 * is enabled:
 *   gale_timer_expiry_handler — wraps gale_timer_expire for status++
 *   gale_timer_status_read   — wraps gale_timer_status_get for read+reset
 *
 * Verified operations (Verus proofs):
 *   gale_timer_expire     — TM5 (increment), TM8 (no overflow)
 *   gale_timer_status_get — TM2 (read + reset to 0)
 *   gale_timer_init_validate — TM6/TM7 (period classification)
 */

#include <zephyr/kernel.h>
#include <zephyr/sys/check.h>

#include "gale_timer.h"

/**
 * Gale-validated timer expiry: increment status with overflow check.
 *
 * Called from the timer expiry handler in kernel/timer.c in place of
 * the bare `timer->status++`.
 *
 * @param timer  Pointer to the timer object.
 *
 * @return 0 on success (status incremented),
 *         -EOVERFLOW if status was at UINT32_MAX (status unchanged).
 */
int gale_timer_expiry_handler(struct k_timer *timer)
{
	uint32_t new_status;
	int32_t ret;

	ret = gale_timer_expire(timer->status, &new_status);
	if (ret == 0) {
		timer->status = new_status;
	}
	/* On overflow, status is left unchanged (saturated at UINT32_MAX) */

	return ret;
}

/**
 * Gale-validated timer status read: return old status and reset to 0.
 *
 * Called from k_timer_status_get in kernel/timer.c in place of the
 * bare read-and-reset sequence.
 *
 * @param timer  Pointer to the timer object.
 *
 * @return The number of expiry events since the last status read
 *         (or since start/stop).  Timer status is reset to 0.
 */
uint32_t gale_timer_status_read(struct k_timer *timer)
{
	uint32_t new_status;
	uint32_t old;

	old = gale_timer_status_get(timer->status, &new_status);
	timer->status = new_status;

	return old;
}
