/*
 * Copyright (c) 2010-2016 Wind River Systems, Inc.
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale queue — phase 2: Extract→Decide→Apply pattern.
 *
 * This is kernel/queue.c with queue_insert and k_queue_get rewritten
 * to use Rust decision structs.  C extracts kernel state (spinlock,
 * wait queue side effects), Rust decides the action, C applies it.
 *
 * The linked list (sys_sflist), alloc nodes, polling, and tracing
 * remain native Zephyr.
 *
 * Verified operations (Verus proofs):
 *   gale_k_queue_insert_decide — QU1-QU4 (wake vs insert decision)
 *   gale_k_queue_get_decide    — QU5/QU6 (dequeue vs null vs pend decision)
 */

#include <zephyr/kernel.h>
#include <zephyr/kernel_structs.h>

#include <zephyr/toolchain.h>
#include <wait_q.h>
#include <ksched.h>
#include <zephyr/init.h>
#include <zephyr/internal/syscall_handler.h>
#include <kernel_internal.h>
#include <zephyr/sys/check.h>

#include "gale_queue.h"

struct alloc_node {
	sys_sfnode_t node;
	void *data;
};

void *z_queue_node_peek(sys_sfnode_t *node, bool needs_free)
{
	void *ret;

	if ((node != NULL) && (sys_sfnode_flags_get(node) != (uint8_t)0)) {
		struct alloc_node *anode;

		anode = CONTAINER_OF(node, struct alloc_node, node);
		ret = anode->data;
		if (needs_free) {
			k_free(anode);
		}
	} else {
		ret = (void *)node;
	}

	return ret;
}

void z_impl_k_queue_init(struct k_queue *queue)
{
	sys_sflist_init(&queue->data_q);
	queue->lock = (struct k_spinlock) {};
	z_waitq_init(&queue->wait_q);
#if defined(CONFIG_POLL)
	sys_dlist_init(&queue->poll_events);
#endif

	SYS_PORT_TRACING_OBJ_INIT(k_queue, queue);

	k_object_init(queue);
}

#ifdef CONFIG_USERSPACE
static inline void z_vrfy_k_queue_init(struct k_queue *queue)
{
	K_OOPS(K_SYSCALL_OBJ_NEVER_INIT(queue, K_OBJ_QUEUE));
	z_impl_k_queue_init(queue);
}
#include <zephyr/syscalls/k_queue_init_mrsh.c>
#endif /* CONFIG_USERSPACE */

static void prepare_thread_to_run(struct k_thread *thread, void *data)
{
	z_thread_return_value_set_with_data(thread, 0, data);
	z_ready_thread(thread);
}

static inline bool handle_poll_events(struct k_queue *queue, uint32_t state)
{
#ifdef CONFIG_POLL
	return z_handle_obj_poll_events(&queue->poll_events, state);
#else
	ARG_UNUSED(queue);
	ARG_UNUSED(state);

	return false;
#endif /* CONFIG_POLL */
}

void z_impl_k_queue_cancel_wait(struct k_queue *queue)
{
	SYS_PORT_TRACING_OBJ_FUNC(k_queue, cancel_wait, queue);

	k_spinlock_key_t key = k_spin_lock(&queue->lock);
	struct k_thread *first_pending_thread;
	bool resched = false;

	first_pending_thread = z_unpend_first_thread(&queue->wait_q);

	if (first_pending_thread != NULL) {
		resched = true;
		prepare_thread_to_run(first_pending_thread, NULL);
	}

	resched = handle_poll_events(queue, K_POLL_STATE_CANCELLED) || resched;

	if (resched) {
		z_reschedule(&queue->lock, key);
	} else {
		k_spin_unlock(&queue->lock, key);
	}
}

#ifdef CONFIG_USERSPACE
static inline void z_vrfy_k_queue_cancel_wait(struct k_queue *queue)
{
	K_OOPS(K_SYSCALL_OBJ(queue, K_OBJ_QUEUE));
	z_impl_k_queue_cancel_wait(queue);
}
#include <zephyr/syscalls/k_queue_cancel_wait_mrsh.c>
#endif /* CONFIG_USERSPACE */

static int32_t queue_insert(struct k_queue *queue, void *prev, void *data,
			    bool alloc, bool is_append)
{
	k_spinlock_key_t key = k_spin_lock(&queue->lock);
	int32_t result = 0;
	bool resched = false;

	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_queue, queue_insert, queue, alloc);

	if (is_append) {
		prev = sys_sflist_peek_tail(&queue->data_q);
	}

	/* Extract: try to unpend first waiter (side effect: removes from queue) */
	struct k_thread *first_pending_thread =
		z_unpend_first_thread(&queue->wait_q);

	/* Decide: Rust determines action based on whether a waiter was found */
	struct gale_queue_insert_decision d = gale_k_queue_insert_decide(
		first_pending_thread != NULL ? 1U : 0U);

	/* Apply: execute Rust's decision */
	if (d.action == GALE_QUEUE_ACTION_WAKE) {
		SYS_PORT_TRACING_OBJ_FUNC_BLOCKING(k_queue, queue_insert, queue, alloc, K_FOREVER);

		prepare_thread_to_run(first_pending_thread, data);
		resched = true;
	} else {
		/* INSERT_INTO_LIST: allocate node if needed, then insert */
		if (alloc) {
			struct alloc_node *anode;

			anode = z_thread_malloc(sizeof(*anode));
			if (anode == NULL) {
				result = -ENOMEM;
				goto out;
			}
			anode->data = data;
			sys_sfnode_init(&anode->node, 0x1);
			data = anode;
		} else {
			sys_sfnode_init(data, 0x0);
		}

		SYS_PORT_TRACING_OBJ_FUNC_BLOCKING(k_queue, queue_insert, queue, alloc, K_FOREVER);

		sys_sflist_insert(&queue->data_q, prev, data);
		resched = handle_poll_events(queue, K_POLL_STATE_DATA_AVAILABLE);
	}

out:
	if (resched) {
		z_reschedule(&queue->lock, key);
	} else {
		k_spin_unlock(&queue->lock, key);
	}

	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_queue, queue_insert, queue, alloc, result);

	return result;
}

void k_queue_insert(struct k_queue *queue, void *prev, void *data)
{
	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_queue, insert, queue);

	(void)queue_insert(queue, prev, data, false, false);

	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_queue, insert, queue);
}

void k_queue_append(struct k_queue *queue, void *data)
{
	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_queue, append, queue);

	(void)queue_insert(queue, NULL, data, false, true);

	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_queue, append, queue);
}

void k_queue_prepend(struct k_queue *queue, void *data)
{
	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_queue, prepend, queue);

	(void)queue_insert(queue, NULL, data, false, false);

	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_queue, prepend, queue);
}

int32_t z_impl_k_queue_alloc_append(struct k_queue *queue, void *data)
{
	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_queue, alloc_append, queue);

	int32_t ret = queue_insert(queue, NULL, data, true, true);

	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_queue, alloc_append, queue, ret);

	return ret;
}

#ifdef CONFIG_USERSPACE
static inline int32_t z_vrfy_k_queue_alloc_append(struct k_queue *queue,
						  void *data)
{
	K_OOPS(K_SYSCALL_OBJ(queue, K_OBJ_QUEUE));
	return z_impl_k_queue_alloc_append(queue, data);
}
#include <zephyr/syscalls/k_queue_alloc_append_mrsh.c>
#endif /* CONFIG_USERSPACE */

int32_t z_impl_k_queue_alloc_prepend(struct k_queue *queue, void *data)
{
	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_queue, alloc_prepend, queue);

	int32_t ret = queue_insert(queue, NULL, data, true, false);

	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_queue, alloc_prepend, queue, ret);

	return ret;
}

#ifdef CONFIG_USERSPACE
static inline int32_t z_vrfy_k_queue_alloc_prepend(struct k_queue *queue,
						   void *data)
{
	K_OOPS(K_SYSCALL_OBJ(queue, K_OBJ_QUEUE));
	return z_impl_k_queue_alloc_prepend(queue, data);
}
#include <zephyr/syscalls/k_queue_alloc_prepend_mrsh.c>
#endif /* CONFIG_USERSPACE */

int k_queue_append_list(struct k_queue *queue, void *head, void *tail)
{
	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_queue, append_list, queue);
	bool resched = false;

	/* invalid head or tail of list */
	CHECKIF((head == NULL) || (tail == NULL)) {
		SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_queue, append_list, queue, -EINVAL);

		return -EINVAL;
	}

	k_spinlock_key_t key = k_spin_lock(&queue->lock);
	struct k_thread *thread = NULL;

	if (head != NULL) {
		thread = z_unpend_first_thread(&queue->wait_q);
	}

	while ((head != NULL) && (thread != NULL)) {
		resched = true;
		prepare_thread_to_run(thread, head);
		head = *(void **)head;
		thread = z_unpend_first_thread(&queue->wait_q);
	}

	if (head != NULL) {
		sys_sflist_append_list(&queue->data_q, head, tail);
	}

	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_queue, append_list, queue, 0);

	resched = handle_poll_events(queue, K_POLL_STATE_DATA_AVAILABLE) || resched;

	if (resched) {
		z_reschedule(&queue->lock, key);
	} else {
		k_spin_unlock(&queue->lock, key);
	}

	return 0;
}

int k_queue_merge_slist(struct k_queue *queue, sys_slist_t *list)
{
	int ret;

	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_queue, merge_slist, queue);

	/* list must not be empty */
	CHECKIF(sys_slist_is_empty(list)) {
		SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_queue, merge_slist, queue, -EINVAL);

		return -EINVAL;
	}

	ret = k_queue_append_list(queue, list->head, list->tail);
	CHECKIF(ret != 0) {
		SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_queue, merge_slist, queue, ret);

		return ret;
	}
	sys_slist_init(list);

	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_queue, merge_slist, queue, 0);

	return 0;
}

void *z_impl_k_queue_get(struct k_queue *queue, k_timeout_t timeout)
{
	k_spinlock_key_t key = k_spin_lock(&queue->lock);
	void *data;

	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_queue, get, queue, timeout);

	/* Extract: check if list has data */
	uint32_t has_data = !sys_sflist_is_empty(&queue->data_q) ? 1U : 0U;

	/* Decide: Rust determines action */
	struct gale_queue_get_decision d = gale_k_queue_get_decide(
		has_data, K_TIMEOUT_EQ(timeout, K_NO_WAIT) ? 1U : 0U);

	/* Apply: execute Rust's decision */
	if (d.action == GALE_QUEUE_ACTION_DEQUEUE) {
		sys_sfnode_t *node;

		node = sys_sflist_get_not_empty(&queue->data_q);
		data = z_queue_node_peek(node, true);
		k_spin_unlock(&queue->lock, key);

		SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_queue, get, queue, timeout, data);

		return data;
	} else if (d.action == GALE_QUEUE_ACTION_RETURN_NULL) {
		k_spin_unlock(&queue->lock, key);

		SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_queue, get, queue, timeout, NULL);

		return NULL;
	} else {
		/* PEND_CURRENT: block on wait queue with timeout */
		SYS_PORT_TRACING_OBJ_FUNC_BLOCKING(k_queue, get, queue, timeout);

		int ret = z_pend_curr(&queue->lock, key, &queue->wait_q, timeout);

		SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_queue, get, queue, timeout,
			(ret != 0) ? NULL : _current->base.swap_data);

		return (ret != 0) ? NULL : _current->base.swap_data;
	}
}

bool k_queue_remove(struct k_queue *queue, void *data)
{
	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_queue, remove, queue);

	bool ret = sys_sflist_find_and_remove(&queue->data_q, (sys_sfnode_t *)data);

	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_queue, remove, queue, ret);

	return ret;
}

bool k_queue_unique_append(struct k_queue *queue, void *data)
{
	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_queue, unique_append, queue);

	sys_sfnode_t *test;

	SYS_SFLIST_FOR_EACH_NODE(&queue->data_q, test) {
		if (test == (sys_sfnode_t *) data) {
			SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_queue, unique_append, queue, false);

			return false;
		}
	}

	k_queue_append(queue, data);

	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_queue, unique_append, queue, true);

	return true;
}

void *z_impl_k_queue_peek_head(struct k_queue *queue)
{
	void *ret = z_queue_node_peek(sys_sflist_peek_head(&queue->data_q), false);

	SYS_PORT_TRACING_OBJ_FUNC(k_queue, peek_head, queue, ret);

	return ret;
}

void *z_impl_k_queue_peek_tail(struct k_queue *queue)
{
	void *ret = z_queue_node_peek(sys_sflist_peek_tail(&queue->data_q), false);

	SYS_PORT_TRACING_OBJ_FUNC(k_queue, peek_tail, queue, ret);

	return ret;
}

#ifdef CONFIG_USERSPACE
static inline void *z_vrfy_k_queue_get(struct k_queue *queue,
				       k_timeout_t timeout)
{
	K_OOPS(K_SYSCALL_OBJ(queue, K_OBJ_QUEUE));
	return z_impl_k_queue_get(queue, timeout);
}
#include <zephyr/syscalls/k_queue_get_mrsh.c>

static inline int z_vrfy_k_queue_is_empty(struct k_queue *queue)
{
	K_OOPS(K_SYSCALL_OBJ(queue, K_OBJ_QUEUE));
	return z_impl_k_queue_is_empty(queue);
}
#include <zephyr/syscalls/k_queue_is_empty_mrsh.c>

static inline void *z_vrfy_k_queue_peek_head(struct k_queue *queue)
{
	K_OOPS(K_SYSCALL_OBJ(queue, K_OBJ_QUEUE));
	return z_impl_k_queue_peek_head(queue);
}
#include <zephyr/syscalls/k_queue_peek_head_mrsh.c>

static inline void *z_vrfy_k_queue_peek_tail(struct k_queue *queue)
{
	K_OOPS(K_SYSCALL_OBJ(queue, K_OBJ_QUEUE));
	return z_impl_k_queue_peek_tail(queue);
}
#include <zephyr/syscalls/k_queue_peek_tail_mrsh.c>

#endif /* CONFIG_USERSPACE */

#ifdef CONFIG_OBJ_CORE_FIFO
struct k_obj_type _obj_type_fifo;

static int init_fifo_obj_core_list(void)
{
	/* Initialize fifo object type */

	z_obj_type_init(&_obj_type_fifo, K_OBJ_TYPE_FIFO_ID,
			offsetof(struct k_fifo, obj_core));

	/* Initialize and link statically defined fifos */

	STRUCT_SECTION_FOREACH(k_fifo, fifo) {
		k_obj_core_init_and_link(K_OBJ_CORE(fifo), &_obj_type_fifo);
	}

	return 0;
}

SYS_INIT(init_fifo_obj_core_list, PRE_KERNEL_1,
	 CONFIG_KERNEL_INIT_PRIORITY_OBJECTS);
#endif /* CONFIG_OBJ_CORE_FIFO */

#ifdef CONFIG_OBJ_CORE_LIFO
struct k_obj_type _obj_type_lifo;

static int init_lifo_obj_core_list(void)
{
	/* Initialize lifo object type */

	z_obj_type_init(&_obj_type_lifo, K_OBJ_TYPE_LIFO_ID,
			offsetof(struct k_lifo, obj_core));

	/* Initialize and link statically defined lifo */

	STRUCT_SECTION_FOREACH(k_lifo, lifo) {
		k_obj_core_init_and_link(K_OBJ_CORE(lifo), &_obj_type_lifo);
	}

	return 0;
}

SYS_INIT(init_lifo_obj_core_list, PRE_KERNEL_1,
	 CONFIG_KERNEL_INIT_PRIORITY_OBJECTS);
#endif /* CONFIG_OBJ_CORE_LIFO */
