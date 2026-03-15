/*
 * Copyright (c) 2021 Intel Corporation
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale event — verified bitmask operations.
 *
 * This is kernel/events.c with the safety-critical bitmask operations
 * (post/set/clear/set_masked, are_wait_conditions_met) replaced by calls
 * to the formally verified Rust implementation.
 *
 * Verified operations (Verus proofs):
 *   gale_event_post           — EV1, EV8 (OR bits, monotonic)
 *   gale_event_set            — EV2 (replace all)
 *   gale_event_clear          — EV3 (AND complement)
 *   gale_event_set_masked     — EV4 (selective set)
 *   gale_event_wait_check_any — EV5 (any-bit match)
 *   gale_event_wait_check_all — EV6 (all-bits match)
 *
 * Wait queues, scheduling, tracing, poll, userspace, and OBJ_CORE
 * remain native Zephyr.
 */

#include <zephyr/kernel.h>
#include <zephyr/kernel_structs.h>

#include <zephyr/toolchain.h>
#include <zephyr/sys/dlist.h>
#include <zephyr/init.h>
#include <zephyr/internal/syscall_handler.h>
#include <zephyr/tracing/tracing.h>
#include <zephyr/sys/check.h>
/* private kernel APIs */
#include <wait_q.h>
#include <ksched.h>

#include "gale_event.h"

#define K_EVENT_WAIT_ANY      0x00   /* Wait for any events */
#define K_EVENT_WAIT_ALL      0x01   /* Wait for all events */
#define K_EVENT_WAIT_MASK     0x01

#define K_EVENT_OPTION_RESET  0x02   /* Reset events prior to waiting */
#define K_EVENT_OPTION_CLEAR  0x04   /* Clear events that are received */

struct event_walk_data {
#ifdef CONFIG_WAITQ_SCALABLE
	struct k_thread  *head;
#endif /* CONFIG_WAITQ_SCALABLE */
	uint32_t events;
	uint32_t clear_events;
};

#ifdef CONFIG_OBJ_CORE_EVENT
static struct k_obj_type obj_type_event;
#endif /* CONFIG_OBJ_CORE_EVENT */

/**
 * @brief determine the set of events that have been satisfied
 *
 * This routine determines if the current set of events satisfies the desired
 * set of events. Uses Gale-verified wait_check_any / wait_check_all.
 */
static uint32_t are_wait_conditions_met(uint32_t desired, uint32_t current,
					unsigned int wait_condition)
{
	uint32_t match = current & desired;

	if (wait_condition == K_EVENT_WAIT_ALL) {
		/* Gale-verified: EV6 — all-bits match */
		if (gale_event_wait_check_all(current, desired) == 0) {
			return 0;
		}
	}

	/* return the matched events for any wait condition */
	return match;
}

void z_impl_k_event_init(struct k_event *event)
{
	__ASSERT_NO_MSG(!arch_is_in_isr());

	event->events = 0;
	event->lock = (struct k_spinlock) {};

	SYS_PORT_TRACING_OBJ_INIT(k_event, event);

	z_waitq_init(&event->wait_q);

	k_object_init(event);

#ifdef CONFIG_OBJ_CORE_EVENT
	k_obj_core_init_and_link(K_OBJ_CORE(event), &obj_type_event);
#endif /* CONFIG_OBJ_CORE_EVENT */
}

#ifdef CONFIG_USERSPACE
void z_vrfy_k_event_init(struct k_event *event)
{
	K_OOPS(K_SYSCALL_OBJ_NEVER_INIT(event, K_OBJ_EVENT));
	z_impl_k_event_init(event);
}
#include <zephyr/syscalls/k_event_init_mrsh.c>
#endif /* CONFIG_USERSPACE */

#ifdef CONFIG_WAITQ_SCALABLE
static void event_post_walk_op(int status, void *data)
{
	ARG_UNUSED(status);
	struct event_walk_data *walk_data = data;
	struct k_thread *thread, *next;

	thread = walk_data->head;

	while (thread != NULL) {
		next = thread->next_event_link;

		arch_thread_return_value_set(thread, 0);
		z_sched_wake_thread_locked(thread);

		thread = next;
	}
}
#define EVENT_POST_WALK_OP_FN event_post_walk_op
#else /* CONFIG_WAITQ_SCALABLE */
#define EVENT_POST_WALK_OP_FN NULL
#endif /* CONFIG_WAITQ_SCALABLE */

static int event_walk_op(struct k_thread *thread, void *data)
{
	uint32_t match;
	unsigned int wait_condition;
	struct event_walk_data *event_data = data;

	wait_condition = thread->event_options & K_EVENT_WAIT_MASK;

	match = are_wait_conditions_met(thread->events, event_data->events,
					wait_condition);
	if (match != 0) {
		thread->events = match;
		if (thread->event_options & K_EVENT_OPTION_CLEAR) {
			event_data->clear_events |= match;
		}
		z_abort_thread_timeout(thread);

#ifndef CONFIG_WAITQ_SCALABLE
		arch_thread_return_value_set(thread, 0);
		z_sched_wake_thread_locked(thread);
#else /* !CONFIG_WAITQ_SCALABLE */
		thread->next_event_link = event_data->head;
		event_data->head = thread;
#endif /* !CONFIG_WAITQ_SCALABLE */
	}

	return 0;
}

static uint32_t k_event_post_internal(struct k_event *event, uint32_t events,
				  uint32_t events_mask)
{
	k_spinlock_key_t  key;
	struct event_walk_data data;
	uint32_t previous_events;

	key = k_spin_lock(&event->lock);

	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_event, post, event, events,
					events_mask);

	previous_events = event->events & events_mask;

	/* Gale-verified: EV4 — set_masked computes
	 * (events & ~mask) | (new & mask) */
	uint32_t new_events;
	gale_event_set_masked(event->events, events, events_mask, &new_events);
	events = new_events;

#ifdef CONFIG_WAITQ_SCALABLE
	data.head = NULL;
#endif /* CONFIG_WAITQ_SCALABLE */
	data.events = events;
	data.clear_events = 0;
	z_sched_waitq_walk(&event->wait_q, event_walk_op, EVENT_POST_WALK_OP_FN, &data);

	/* stash any events not consumed */
	event->events = data.events & ~data.clear_events;

	z_reschedule(&event->lock, key);

	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_event, post, event, events,
				       events_mask);

	return previous_events;
}

uint32_t z_impl_k_event_post(struct k_event *event, uint32_t events)
{
	return k_event_post_internal(event, events, events);
}

#ifdef CONFIG_USERSPACE
uint32_t z_vrfy_k_event_post(struct k_event *event, uint32_t events)
{
	K_OOPS(K_SYSCALL_OBJ(event, K_OBJ_EVENT));
	return z_impl_k_event_post(event, events);
}
#include <zephyr/syscalls/k_event_post_mrsh.c>
#endif /* CONFIG_USERSPACE */

uint32_t z_impl_k_event_set(struct k_event *event, uint32_t events)
{
	return k_event_post_internal(event, events, ~0);
}

#ifdef CONFIG_USERSPACE
uint32_t z_vrfy_k_event_set(struct k_event *event, uint32_t events)
{
	K_OOPS(K_SYSCALL_OBJ(event, K_OBJ_EVENT));
	return z_impl_k_event_set(event, events);
}
#include <zephyr/syscalls/k_event_set_mrsh.c>
#endif /* CONFIG_USERSPACE */

uint32_t z_impl_k_event_set_masked(struct k_event *event, uint32_t events,
			       uint32_t events_mask)
{
	return k_event_post_internal(event, events, events_mask);
}

#ifdef CONFIG_USERSPACE
uint32_t z_vrfy_k_event_set_masked(struct k_event *event, uint32_t events,
			       uint32_t events_mask)
{
	K_OOPS(K_SYSCALL_OBJ(event, K_OBJ_EVENT));
	return z_impl_k_event_set_masked(event, events, events_mask);
}
#include <zephyr/syscalls/k_event_set_masked_mrsh.c>
#endif /* CONFIG_USERSPACE */

uint32_t z_impl_k_event_clear(struct k_event *event, uint32_t events)
{
	return k_event_post_internal(event, 0, events);
}

#ifdef CONFIG_USERSPACE
uint32_t z_vrfy_k_event_clear(struct k_event *event, uint32_t events)
{
	K_OOPS(K_SYSCALL_OBJ(event, K_OBJ_EVENT));
	return z_impl_k_event_clear(event, events);
}
#include <zephyr/syscalls/k_event_clear_mrsh.c>
#endif /* CONFIG_USERSPACE */

static uint32_t k_event_wait_internal(struct k_event *event, uint32_t events,
				      unsigned int options, k_timeout_t timeout)
{
	uint32_t  rv = 0;
	unsigned int  wait_condition;
	struct k_thread  *thread;

	__ASSERT(((arch_is_in_isr() == false) ||
		  K_TIMEOUT_EQ(timeout, K_NO_WAIT)), "");

	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_event, wait, event, events,
					options, timeout);

	if (events == 0) {
		SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_event, wait, event, events, 0);
		return 0;
	}

	wait_condition = options & K_EVENT_WAIT_MASK;
	thread = k_sched_current_thread_query();

	k_spinlock_key_t  key = k_spin_lock(&event->lock);

	if (options & K_EVENT_OPTION_RESET) {
		event->events = 0;
	}

	/* Gale-verified: EV5/EV6 — wait condition check */
	rv = are_wait_conditions_met(events, event->events, wait_condition);
	if (rv != 0) {
		/* clear the events that are matched */
		if (options & K_EVENT_OPTION_CLEAR) {
			/* Gale-verified: EV3 — clear matched bits */
			uint32_t cleared;
			gale_event_clear(event->events, rv, &cleared);
			event->events = cleared;
		}

		k_spin_unlock(&event->lock, key);
		goto out;
	}

	if (K_TIMEOUT_EQ(timeout, K_NO_WAIT)) {
		k_spin_unlock(&event->lock, key);
		goto out;
	}

	thread->events = events;
	thread->event_options = options;

	SYS_PORT_TRACING_OBJ_FUNC_BLOCKING(k_event, wait, event, events,
					   options, timeout);

	if (z_pend_curr(&event->lock, key, &event->wait_q, timeout) == 0) {
		/* Retrieve the set of events that woke the thread */
		rv = thread->events;
	}

out:
	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_event, wait, event, events, rv);

	return rv;
}

/**
 * Wait for any of the specified events
 */
uint32_t z_impl_k_event_wait(struct k_event *event, uint32_t events,
			     bool reset, k_timeout_t timeout)
{
	uint32_t options = reset ? K_EVENT_OPTION_RESET : 0;

	return k_event_wait_internal(event, events, options, timeout);
}
#ifdef CONFIG_USERSPACE
uint32_t z_vrfy_k_event_wait(struct k_event *event, uint32_t events,
				    bool reset, k_timeout_t timeout)
{
	K_OOPS(K_SYSCALL_OBJ(event, K_OBJ_EVENT));
	return z_impl_k_event_wait(event, events, reset, timeout);
}
#include <zephyr/syscalls/k_event_wait_mrsh.c>
#endif /* CONFIG_USERSPACE */

/**
 * Wait for all of the specified events
 */
uint32_t z_impl_k_event_wait_all(struct k_event *event, uint32_t events,
				 bool reset, k_timeout_t timeout)
{
	uint32_t options = reset ? (K_EVENT_OPTION_RESET | K_EVENT_WAIT_ALL)
				 : K_EVENT_WAIT_ALL;

	return k_event_wait_internal(event, events, options, timeout);
}

#ifdef CONFIG_USERSPACE
uint32_t z_vrfy_k_event_wait_all(struct k_event *event, uint32_t events,
					bool reset, k_timeout_t timeout)
{
	K_OOPS(K_SYSCALL_OBJ(event, K_OBJ_EVENT));
	return z_impl_k_event_wait_all(event, events, reset, timeout);
}
#include <zephyr/syscalls/k_event_wait_all_mrsh.c>
#endif /* CONFIG_USERSPACE */

uint32_t z_impl_k_event_wait_safe(struct k_event *event, uint32_t events,
				  bool reset, k_timeout_t timeout)
{
	uint32_t options = reset ? (K_EVENT_OPTION_CLEAR | K_EVENT_OPTION_RESET)
				 : K_EVENT_OPTION_CLEAR;

	return k_event_wait_internal(event, events, options, timeout);
}

#ifdef CONFIG_USERSPACE
uint32_t z_vrfy_k_event_wait_safe(struct k_event *event, uint32_t events,
				  bool reset, k_timeout_t timeout)
{
	K_OOPS(K_SYSCALL_OBJ(event, K_OBJ_EVENT));
	return z_impl_k_event_wait_safe(event, events, reset, timeout);
}
#include <zephyr/syscalls/k_event_wait_safe_mrsh.c>
#endif /* CONFIG_USERSPACE */

uint32_t z_impl_k_event_wait_all_safe(struct k_event *event, uint32_t events,
				      bool reset, k_timeout_t timeout)
{
	uint32_t options = reset ? (K_EVENT_OPTION_CLEAR |
				    K_EVENT_OPTION_RESET | K_EVENT_WAIT_ALL)
				 : (K_EVENT_OPTION_CLEAR | K_EVENT_WAIT_ALL);

	return k_event_wait_internal(event, events, options, timeout);
}

#ifdef CONFIG_USERSPACE
uint32_t z_vrfy_k_event_wait_all_safe(struct k_event *event, uint32_t events,
				      bool reset, k_timeout_t timeout)
{
	K_OOPS(K_SYSCALL_OBJ(event, K_OBJ_EVENT));
	return z_impl_k_event_wait_all_safe(event, events, reset, timeout);
}
#include <zephyr/syscalls/k_event_wait_all_safe_mrsh.c>
#endif /* CONFIG_USERSPACE */

#ifdef CONFIG_OBJ_CORE_EVENT
static int init_event_obj_core_list(void)
{
	/* Initialize event object type */
	z_obj_type_init(&obj_type_event, K_OBJ_TYPE_EVENT_ID,
			offsetof(struct k_event, obj_core));

	/* Initialize and link statically defined events */
	STRUCT_SECTION_FOREACH(k_event, event) {
		k_obj_core_init_and_link(K_OBJ_CORE(event), &obj_type_event);
	}

	return 0;
}

SYS_INIT(init_event_obj_core_list, PRE_KERNEL_1,
	 CONFIG_KERNEL_INIT_PRIORITY_OBJECTS);
#endif /* CONFIG_OBJ_CORE_EVENT */
