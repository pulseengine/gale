/*
 * Copyright (c) 2018 Intel Corporation
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale usage — Extract→Decide→Apply shim for kernel/usage.c.
 *
 * This C shim wraps the thread runtime statistics decision logic from
 * kernel/usage.c with Rust-verified decision functions.
 *
 *   C extracts kernel state (track_usage flag, usage0 snapshot, cpu id)
 *   Rust decides whether to apply state changes and how to compute stats
 *   C applies (writes to thread->base.usage, cpu->usage0, stats output)
 *
 * Timing hardware, spinlocks, per-CPU data structures, and the actual
 * k_cycle_get_32() / timing_counter_get() calls remain in Zephyr.
 *
 * Verified decision functions (Verus proofs):
 *   gale_usage_sys_enable_decide  — US4 (idempotent sys enable)
 *   gale_usage_sys_disable_decide — US4 (idempotent sys disable)
 *   gale_usage_start_decide       — US1 (window tracking guard)
 *   gale_usage_stop_decide        — US2 (accumulate only when usage0 != 0)
 *   gale_usage_average_cycles     — US5 (no division by zero)
 *   gale_usage_elapsed_cycles     — US2 (wrapping u32 subtraction)
 *   gale_usage_accumulate         — US6 (monotone total_cycles)
 */

#include <zephyr/kernel.h>
#include <zephyr/timing/timing.h>
#include <ksched.h>
#include <zephyr/spinlock.h>
#include <zephyr/sys/check.h>
#include <string.h>

#include "gale_usage.h"

/* ------------------------------------------------------------------ */
/* Spinlock — shared with all usage operations                         */
/* ------------------------------------------------------------------ */

static struct k_spinlock gale_usage_lock;

/* ------------------------------------------------------------------ */
/* Internal: read cycle counter (mirrors usage_now() in usage.c)      */
/* ------------------------------------------------------------------ */

static uint32_t gale_usage_now(void)
{
	uint32_t now;

#ifdef CONFIG_THREAD_RUNTIME_STATS_USE_TIMING_FUNCTIONS
	now = (uint32_t)timing_counter_get();
#else
	now = k_cycle_get_32();
#endif

	/* Edge case: zero is used as a null sentinel ("stop already called") */
	return (now == 0U) ? 1U : now;
}

/* ------------------------------------------------------------------ */
/* z_sched_usage_start replacement                                     */
/* ------------------------------------------------------------------ */

/**
 * Record the start timestamp for the current thread's execution window.
 *
 * Extract: thread->base.usage.track_usage
 * Decide:  gale_usage_start_decide()
 * Apply:   cpu->usage0 = now; optionally reset window stats
 *
 * Maps usage.c:74-97.
 */
void z_sched_usage_start(struct k_thread *thread)
{
#ifdef CONFIG_SCHED_THREAD_USAGE_ANALYSIS
	k_spinlock_key_t key = k_spin_lock(&gale_usage_lock);

	uint32_t now = gale_usage_now();

	_current_cpu->usage0 = now; /* Always update */

	/* Decide: does this thread need window accounting? */
	uint8_t action = gale_usage_start_decide(
		(uint32_t)thread->base.usage.track_usage);

	if (action == GALE_USAGE_START_RECORD_WINDOW) {
		thread->base.usage.num_windows++;
		thread->base.usage.current = 0;
	}

	k_spin_unlock(&gale_usage_lock, key);
#else
	/* Without analysis: single volatile write — no lock needed */
	_current_cpu->usage0 = gale_usage_now();
#endif
}

/* ------------------------------------------------------------------ */
/* z_sched_usage_stop replacement                                      */
/* ------------------------------------------------------------------ */

/**
 * Accumulate elapsed cycles for the thread that just stopped running.
 *
 * Extract: cpu->usage0 snapshot, current thread's track_usage
 * Decide:  gale_usage_stop_decide() — skip if usage0 == 0
 *          gale_usage_elapsed_cycles() — wrapping subtraction
 *          gale_usage_accumulate() — overflow-safe accumulation
 * Apply:   thread->base.usage.total += cycles; cpu->usage0 = 0
 *
 * Maps usage.c:99-119.
 */
void z_sched_usage_stop(void)
{
	k_spinlock_key_t key = k_spin_lock(&gale_usage_lock);

	struct _cpu *cpu = _current_cpu;
	uint32_t u0 = cpu->usage0;

	/* Decide: should we accumulate cycles at all? */
	uint8_t action = gale_usage_stop_decide(u0);

	if (action == GALE_USAGE_STOP_ACCUMULATE) {
		uint32_t now    = gale_usage_now();
		uint32_t cycles = gale_usage_elapsed_cycles(now, u0);

		if (cpu->current->base.usage.track_usage) {
			/* Accumulate into thread total (overflow-safe) */
			gale_usage_accumulate(&cpu->current->base.usage.total,
					      cycles);

#ifdef CONFIG_SCHED_THREAD_USAGE_ANALYSIS
			/* Update analysis fields directly (mirrors Zephyr) */
			cpu->current->base.usage.current += cycles;
			if (cpu->current->base.usage.longest <
			    cpu->current->base.usage.current) {
				cpu->current->base.usage.longest =
					cpu->current->base.usage.current;
			}
#endif
		}

#ifdef CONFIG_SCHED_THREAD_USAGE_ALL
		if (cpu->usage->track_usage) {
			cpu->usage->total += cycles;
#ifdef CONFIG_SCHED_THREAD_USAGE_ANALYSIS
			if (cpu->current != cpu->idle_thread) {
				cpu->usage->current += cycles;
				if (cpu->usage->longest < cpu->usage->current) {
					cpu->usage->longest =
						cpu->usage->current;
				}
			} else {
				cpu->usage->current = 0;
				cpu->usage->num_windows++;
			}
#endif
		}
#endif /* CONFIG_SCHED_THREAD_USAGE_ALL */
	}

	cpu->usage0 = 0;
	k_spin_unlock(&gale_usage_lock, key);
}

/* ------------------------------------------------------------------ */
/* z_sched_thread_usage replacement                                    */
/* ------------------------------------------------------------------ */

/**
 * Gather runtime stats for a thread into a stats struct.
 *
 * If the thread is currently running, bring its stats up-to-date first.
 * The average_cycles computation uses gale_usage_average_cycles() to
 * avoid division by zero (US5).
 *
 * Maps usage.c:172-224.
 */
void z_sched_thread_usage(struct k_thread *thread,
			   struct k_thread_runtime_stats *stats)
{
	k_spinlock_key_t key = k_spin_lock(&gale_usage_lock);
	struct _cpu *cpu = _current_cpu;

	if (thread == cpu->current) {
		/*
		 * Thread is running right now — bring stats up to date.
		 * Mirrors the same logic as stop but without zeroing usage0.
		 */
		uint32_t now    = gale_usage_now();
		uint32_t cycles = gale_usage_elapsed_cycles(now, cpu->usage0);

		if (thread->base.usage.track_usage) {
			gale_usage_accumulate(&thread->base.usage.total,
					      cycles);
#ifdef CONFIG_SCHED_THREAD_USAGE_ANALYSIS
			thread->base.usage.current += cycles;
			if (thread->base.usage.longest <
			    thread->base.usage.current) {
				thread->base.usage.longest =
					thread->base.usage.current;
			}
#endif
		}

#ifdef CONFIG_SCHED_THREAD_USAGE_ALL
		if (cpu->usage->track_usage) {
			cpu->usage->total += cycles;
		}
#endif
		cpu->usage0 = now;
	}

	stats->execution_cycles = thread->base.usage.total;
	stats->total_cycles     = thread->base.usage.total;

#ifdef CONFIG_SCHED_THREAD_USAGE_ANALYSIS
	stats->current_cycles = thread->base.usage.current;
	stats->peak_cycles    = thread->base.usage.longest;

	/*
	 * Decide: gale_usage_average_cycles() returns 0 when num_windows == 0
	 * to prevent division by zero (US5).
	 */
	(void)gale_usage_average_cycles(
		thread->base.usage.total,
		thread->base.usage.num_windows,
		&stats->average_cycles);
#endif

#ifdef CONFIG_SCHED_THREAD_USAGE_ALL
	stats->idle_cycles = 0;
#endif

	k_spin_unlock(&gale_usage_lock, key);
}

/* ------------------------------------------------------------------ */
/* k_thread_runtime_stats_enable / disable                             */
/* ------------------------------------------------------------------ */

#ifdef CONFIG_SCHED_THREAD_USAGE_ANALYSIS

/**
 * Enable per-thread runtime stats tracking.
 *
 * Extract: thread->base.usage.track_usage
 * Decide:  implicit (enable is idempotent — no action code needed)
 * Apply:   track_usage = true; num_windows++; current = 0
 *
 * Maps usage.c:227-246.
 */
int k_thread_runtime_stats_enable(k_tid_t thread)
{
	k_spinlock_key_t key;

	CHECKIF(thread == NULL) {
		return -EINVAL;
	}

	key = k_spin_lock(&gale_usage_lock);

	if (!thread->base.usage.track_usage) {
		thread->base.usage.track_usage = true;
		thread->base.usage.num_windows++;
		thread->base.usage.current = 0;
	}

	k_spin_unlock(&gale_usage_lock, key);

	return 0;
}

/**
 * Disable per-thread runtime stats tracking.
 *
 * If the thread is currently running, flush elapsed cycles before
 * disabling to avoid losing the current window's data.
 *
 * Maps usage.c:248-273.
 */
int k_thread_runtime_stats_disable(k_tid_t thread)
{
	k_spinlock_key_t key;

	CHECKIF(thread == NULL) {
		return -EINVAL;
	}

	key = k_spin_lock(&gale_usage_lock);

	struct _cpu *cpu = _current_cpu;

	if (thread->base.usage.track_usage) {
		thread->base.usage.track_usage = false;

		if (thread == cpu->current) {
			/* Flush current window before disabling */
			uint32_t now    = gale_usage_now();
			uint32_t cycles = gale_usage_elapsed_cycles(
				now, cpu->usage0);

			gale_usage_accumulate(&thread->base.usage.total,
					      cycles);
			thread->base.usage.current += cycles;
			if (thread->base.usage.longest <
			    thread->base.usage.current) {
				thread->base.usage.longest =
					thread->base.usage.current;
			}

#ifdef CONFIG_SCHED_THREAD_USAGE_ALL
			if (cpu->usage->track_usage) {
				cpu->usage->total += cycles;
			}
#endif
		}
	}

	k_spin_unlock(&gale_usage_lock, key);

	return 0;
}

#endif /* CONFIG_SCHED_THREAD_USAGE_ANALYSIS */

/* ------------------------------------------------------------------ */
/* k_sys_runtime_stats_enable / disable                                */
/* ------------------------------------------------------------------ */

#ifdef CONFIG_SCHED_THREAD_USAGE_ALL

/**
 * Enable system-wide (all CPU) runtime stats tracking.
 *
 * Extract: current CPU's track_usage flag
 * Decide:  gale_usage_sys_enable_decide() — idempotent guard
 * Apply:   set track_usage on all CPUs
 *
 * Maps usage.c:277-307.
 */
void k_sys_runtime_stats_enable(void)
{
	k_spinlock_key_t key = k_spin_lock(&gale_usage_lock);

	/* Decide: already enabled? */
	uint8_t action = gale_usage_sys_enable_decide(
		(uint32_t)_current_cpu->usage->track_usage);

	if (action == GALE_USAGE_SYS_NOOP) {
		k_spin_unlock(&gale_usage_lock, key);
		return;
	}

	/* Apply: enable on all CPUs */
	unsigned int num_cpus = arch_num_cpus();

	for (uint8_t i = 0; i < num_cpus; i++) {
		_kernel.cpus[i].usage->track_usage = true;
#ifdef CONFIG_SCHED_THREAD_USAGE_ANALYSIS
		_kernel.cpus[i].usage->num_windows++;
		_kernel.cpus[i].usage->current = 0;
#endif
	}

	k_spin_unlock(&gale_usage_lock, key);
}

/**
 * Disable system-wide (all CPU) runtime stats tracking.
 *
 * Flushes any in-progress window before disabling.
 *
 * Extract: current CPU's track_usage flag
 * Decide:  gale_usage_sys_disable_decide() — idempotent guard
 *          gale_usage_elapsed_cycles() — flush current window
 * Apply:   clear track_usage on all CPUs
 *
 * Maps usage.c:310-342.
 */
void k_sys_runtime_stats_disable(void)
{
	k_spinlock_key_t key = k_spin_lock(&gale_usage_lock);

	/* Decide: already disabled? */
	uint8_t action = gale_usage_sys_disable_decide(
		(uint32_t)_current_cpu->usage->track_usage);

	if (action == GALE_USAGE_SYS_NOOP) {
		k_spin_unlock(&gale_usage_lock, key);
		return;
	}

	uint32_t now = gale_usage_now();
	unsigned int num_cpus = arch_num_cpus();

	for (uint8_t i = 0; i < num_cpus; i++) {
		struct _cpu *cpu = &_kernel.cpus[i];

		if (cpu->usage0 != 0U) {
			uint32_t cycles = gale_usage_elapsed_cycles(
				now, cpu->usage0);
			if (cpu->usage->track_usage) {
				cpu->usage->total += cycles;
			}
		}
		cpu->usage->track_usage = false;
	}

	k_spin_unlock(&gale_usage_lock, key);
}

#endif /* CONFIG_SCHED_THREAD_USAGE_ALL */
