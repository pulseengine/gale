/*
 * Copyright (c) 2019 Intel Corporation.
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale fatal — verified fatal error classification.
 *
 * This C shim provides the glue between Zephyr's fatal error subsystem
 * and the formally verified Rust FFI.  IRQ lock, coredump, thread abort,
 * and arch_system_halt remain in Zephyr.
 *
 * Pattern: Extract -> Decide -> Apply
 *   Extract: C reads reason, ISR flag, test mode
 *   Decide:  Rust classifies via gale_k_fatal_decide
 *   Apply:   C executes the recovery action (halt / abort / ignore)
 *
 * Verified operations (Verus proofs):
 *   gale_k_fatal_decide — FT1 (mapping), FT2 (panic halts), FT3 (recovery)
 */

#include <zephyr/kernel.h>

#include <kernel_internal.h>
#include <zephyr/kernel_structs.h>
#include <zephyr/sys/__assert.h>
#include <zephyr/arch/cpu.h>
#include <zephyr/logging/log_ctrl.h>
#include <zephyr/logging/log.h>
#include <zephyr/fatal.h>
#include <zephyr/debug/coredump.h>

#include "gale_fatal.h"

LOG_MODULE_DECLARE(os, CONFIG_KERNEL_LOG_LEVEL);

static const char *thread_name_get(struct k_thread *thread)
{
	const char *thread_name = (thread != NULL) ? k_thread_name_get(thread) : NULL;

	if ((thread_name == NULL) || (thread_name[0] == '\0')) {
		thread_name = "unknown";
	}

	return thread_name;
}

static const char *reason_to_str(unsigned int reason)
{
	switch (reason) {
	case K_ERR_CPU_EXCEPTION:
		return "CPU exception";
	case K_ERR_SPURIOUS_IRQ:
		return "Unhandled interrupt";
	case K_ERR_STACK_CHK_FAIL:
		return "Stack overflow";
	case K_ERR_KERNEL_OOPS:
		return "Kernel oops";
	case K_ERR_KERNEL_PANIC:
		return "Kernel panic";
	default:
		return "Unknown error";
	}
}

FUNC_NORETURN void k_fatal_halt(unsigned int reason)
{
	ARG_UNUSED(reason);
	for (;;) {
		/* spin forever */
	}
}

void z_fatal_error(unsigned int reason, const struct arch_esf *esf)
{
	/* We can't allow this code to be preempted, but don't need to
	 * synchronize between CPUs, so an arch-layer lock is
	 * appropriate.
	 */
	unsigned int key = arch_irq_lock();
	struct k_thread *thread = IS_ENABLED(CONFIG_MULTITHREADING) ?
			_current : NULL;

	/* twister looks for the "ZEPHYR FATAL ERROR" string, don't
	 * change it without also updating twister
	 */
	LOG_ERR(">>> ZEPHYR FATAL ERROR %d: %s on CPU %d", reason,
		reason_to_str(reason), _current_cpu->id);

#if defined(CONFIG_ARCH_HAS_NESTED_EXCEPTION_DETECTION)
	if ((esf != NULL) && arch_is_in_nested_exception(esf)) {
		LOG_ERR("Fault during interrupt handling\n");
	}
#endif /* CONFIG_ARCH_HAS_NESTED_EXCEPTION_DETECTION */

	if (IS_ENABLED(CONFIG_MULTITHREADING)) {
		LOG_ERR("Current thread: %p (%s)", thread,
			thread_name_get(thread));
	}

	coredump(reason, esf, thread);

	k_sys_fatal_error_handler(reason, esf);

	/* ---- Extract ---- */
	uint32_t is_isr = 0U;
#if defined(CONFIG_ARCH_HAS_NESTED_EXCEPTION_DETECTION)
	if ((esf != NULL) && arch_is_in_nested_exception(esf)) {
		is_isr = 1U;
	}
#endif

	uint32_t test_mode = IS_ENABLED(CONFIG_TEST) ? 1U : 0U;

	/* ---- Decide (Rust) ---- */
	struct gale_fatal_decision d = gale_k_fatal_decide(
		(uint32_t)reason, is_isr, test_mode);

	/* ---- Apply ---- */
	if (d.action == GALE_FATAL_ACTION_HALT) {
		if (!IS_ENABLED(CONFIG_TEST)) {
			__ASSERT(reason != K_ERR_KERNEL_PANIC,
				 "Attempted to recover from a kernel panic condition");
#if defined(CONFIG_ARCH_HAS_NESTED_EXCEPTION_DETECTION)
			if (is_isr != 0U) {
#if defined(CONFIG_STACK_SENTINEL)
				if (reason != K_ERR_STACK_CHK_FAIL) {
					__ASSERT(0,
					 "Attempted to recover from a fatal error in ISR");
				}
#endif /* CONFIG_STACK_SENTINEL */
			}
#endif /* CONFIG_ARCH_HAS_NESTED_EXCEPTION_DETECTION */
		}
		/* Fall through to thread abort — Zephyr's default recovery
		 * after the asserts fire (or in test mode halt means abort).
		 */
	} else if (d.action == GALE_FATAL_ACTION_IGNORE) {
		/* Test mode ISR — return without aborting */
		arch_irq_unlock(key);
		return;
	}
	/* GALE_FATAL_ACTION_ABORT_THREAD — fall through to abort */

	arch_irq_unlock(key);

	if (IS_ENABLED(CONFIG_MULTITHREADING)) {
		k_thread_abort(thread);
	}
}
