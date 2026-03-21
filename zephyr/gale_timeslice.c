/*
 * Copyright (c) 2018, 2024 Intel Corporation
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale timeslice — phase 2: Extract→Decide→Apply pattern.
 *
 * This is kernel/timeslicing.c with z_time_slice rewritten to use
 * Rust decision structs. C extracts per-CPU slice state (expiry flag,
 * slice size, cooperative status), Rust decides whether to yield,
 * C applies the result (move to end of prio queue + reset).
 *
 * Timeout scheduling, IPI dispatch, and per-CPU arrays remain native
 * Zephyr C.
 *
 * Verified operations (Verus proofs):
 *   gale_k_timeslice_tick_decide — TS4 (expire), TS6 (cooperative no-yield)
 *   gale_timeslice_reset         — TS2 (reset to max)
 *   gale_timeslice_tick           — TS3 (decrement), TS4 (expire), TS5 (no underflow)
 */

#include <zephyr/kernel.h>
#include <kswap.h>
#include <ksched.h>
#include <ipi.h>

#include "gale_timeslice.h"

static int slice_ticks = DIV_ROUND_UP(CONFIG_TIMESLICE_SIZE * Z_HZ_ticks, Z_HZ_ms);
static int slice_max_prio = CONFIG_TIMESLICE_PRIORITY;
static struct _timeout slice_timeouts[CONFIG_MP_MAX_NUM_CPUS];
static bool slice_expired[CONFIG_MP_MAX_NUM_CPUS];

#ifdef CONFIG_SWAP_NONATOMIC
/* If z_swap() isn't atomic, then it's possible for a timer interrupt
 * to try to timeslice away _current after it has already pended
 * itself but before the corresponding context switch.  Treat that as
 * a noop condition in z_time_slice().
 */
struct k_thread *pending_current;
#endif

static inline int slice_time(struct k_thread *thread)
{
	int ret = slice_ticks;

#ifdef CONFIG_TIMESLICE_PER_THREAD
	if (thread->base.slice_ticks != 0) {
		ret = thread->base.slice_ticks;
	}
#else
	ARG_UNUSED(thread);
#endif
	return ret;
}

static int z_time_slice_size(struct k_thread *thread)
{
	if (z_is_thread_prevented_from_running(thread) ||
	    z_is_idle_thread_object(thread) ||
	    (slice_time(thread) == 0)) {
		return 0;
	}

#ifdef CONFIG_TIMESLICE_PER_THREAD
	if (thread->base.slice_ticks != 0) {
		return thread->base.slice_ticks;
	}
#endif

	if (thread_is_preemptible(thread) &&
	    !z_is_prio_higher(thread->base.prio, slice_max_prio)) {
		return slice_ticks;
	}

	return 0;
}

static void slice_timeout(struct _timeout *timeout)
{
	int cpu = ARRAY_INDEX(slice_timeouts, timeout);

	slice_expired[cpu] = true;

	/* We need an IPI if we just handled a timeslice expiration
	 * for a different CPU.
	 */
	if (cpu != _current_cpu->id) {
		flag_ipi(IPI_CPU_MASK(cpu));
	}
}

void z_reset_time_slice(struct k_thread *thread)
{
	int cpu = _current_cpu->id;
	int slice_size = z_time_slice_size(thread);

	z_abort_timeout(&slice_timeouts[cpu]);
	slice_expired[cpu] = false;
	if (slice_size != 0) {
		z_add_timeout(&slice_timeouts[cpu], slice_timeout,
			      K_TICKS(slice_size - 1));
	}
}

static ALWAYS_INLINE bool thread_defines_time_slice_size(struct k_thread *thread)
{
#ifdef CONFIG_TIMESLICE_PER_THREAD
	return (thread->base.slice_ticks != 0);
#else  /* !CONFIG_TIMESLICE_PER_THREAD */
	return false;
#endif /* !CONFIG_TIMESLICE_PER_THREAD */
}

void k_sched_time_slice_set(int32_t slice, int prio)
{
	k_spinlock_key_t key = k_spin_lock(&_sched_spinlock);

	slice_ticks = k_ms_to_ticks_ceil32(slice);
	slice_max_prio = prio;

	/*
	 * Threads that define their own time slice size should not have
	 * their time slices reset here as a thread-specific time slice size
	 * takes precedence over the global time slice size.
	 */

	if (!thread_defines_time_slice_size(_current)) {
		z_reset_time_slice(_current);
	}

	k_spin_unlock(&_sched_spinlock, key);
}

#ifdef CONFIG_TIMESLICE_PER_THREAD
void k_thread_time_slice_set(struct k_thread *thread, int32_t thread_slice_ticks,
			     k_thread_timeslice_fn_t expired, void *data)
{
	K_SPINLOCK(&_sched_spinlock) {
		thread->base.slice_ticks = thread_slice_ticks;
		thread->base.slice_expired = expired;
		thread->base.slice_data = data;
		z_reset_time_slice(thread);
	}
}
#endif

/* Called out of each timer and IPI interrupt.
 *
 * Extract→Decide→Apply: C extracts the per-CPU slice state and thread
 * flags, Rust decides whether to yield, C applies the result.
 */
void z_time_slice(void)
{
	k_spinlock_key_t key = k_spin_lock(&_sched_spinlock);
	struct k_thread *curr = _current;

#ifdef CONFIG_SWAP_NONATOMIC
	if (pending_current == curr) {
		z_reset_time_slice(curr);
		k_spin_unlock(&_sched_spinlock, key);
		return;
	}
	pending_current = NULL;
#endif

	/* Extract: gather slice state from per-CPU arrays and thread */
	uint32_t expired = slice_expired[_current_cpu->id] ? 1U : 0U;
	uint32_t ss = (uint32_t)z_time_slice_size(curr);
	uint32_t cooperative = thread_is_preemptible(curr) ? 0U : 1U;

	/* Decide: Rust determines whether to yield */
	struct gale_timeslice_tick_decision d =
		gale_k_timeslice_tick_decide(
			expired ? 0U : 1U,   /* ticks_remaining: 0 if expired */
			ss,                  /* slice_ticks for this thread    */
			cooperative);        /* cooperative flag               */

	/* Apply: execute Rust's decision */
	if (d.action == GALE_TIMESLICE_ACTION_YIELD) {
#ifdef CONFIG_TIMESLICE_PER_THREAD
		k_thread_timeslice_fn_t handler = curr->base.slice_expired;

		if (handler != NULL) {
			k_spin_unlock(&_sched_spinlock, key);
			handler(curr, curr->base.slice_data);
			key = k_spin_lock(&_sched_spinlock);
		}
#endif
		if (!z_is_thread_prevented_from_running(curr)) {
			move_current_to_end_of_prio_q();
		}
		z_reset_time_slice(curr);
	}

	k_spin_unlock(&_sched_spinlock, key);
}
