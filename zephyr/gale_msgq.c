/*
 * Gale-verified message queue — drop-in replacement for kernel/msg_q.c.
 *
 * Ring buffer index arithmetic is delegated to verified Rust FFI functions.
 * All scheduling, wait queue, memcpy, polling, and tracing remain native C.
 *
 * Slot-to-pointer conversion:
 *   byte_ptr = buffer_start + slot_idx * msg_size
 *   slot_idx = (byte_ptr - buffer_start) / msg_size
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#include <zephyr/kernel.h>
#include <zephyr/kernel_structs.h>
#include <zephyr/internal/syscall_handler.h>
#include <zephyr/sys/check.h>

#include <zephyr/toolchain.h>
#include <zephyr/linker/sections.h>
#include <string.h>
#include <ksched.h>
#include <wait_q.h>

#include "gale_msgq.h"

/* -----------------------------------------------------------------------
 * Helper: convert between slot indices and byte pointers
 * ----------------------------------------------------------------------- */

static inline uint32_t ptr_to_slot(struct k_msgq *msgq, char *ptr)
{
	return (uint32_t)((ptr - msgq->buffer_start) / msgq->msg_size);
}

static inline char *slot_to_ptr(struct k_msgq *msgq, uint32_t slot)
{
	return msgq->buffer_start + ((size_t)slot * msgq->msg_size);
}

/* -----------------------------------------------------------------------
 * k_msgq_init  (replaces msg_q.c:43-71)
 * ----------------------------------------------------------------------- */

void k_msgq_init(struct k_msgq *msgq, char *buffer,
		 size_t msg_size, uint32_t max_msgs)
{
	uint32_t buffer_size = 0;

	/* Validate with verified Rust. */
	int ret = gale_msgq_init_validate((uint32_t)msg_size, max_msgs,
					  &buffer_size);
	__ASSERT(ret == 0, "gale_msgq_init_validate failed: %d", ret);
	ARG_UNUSED(ret);

	msgq->msg_size = msg_size;
	msgq->max_msgs = max_msgs;
	msgq->buffer_start = buffer;
	msgq->buffer_end = buffer + buffer_size;
	msgq->read_ptr = buffer;
	msgq->write_ptr = buffer;
	msgq->used_msgs = 0U;
	msgq->flags = 0;

	z_waitq_init(&msgq->wait_q);

	SYS_PORT_TRACING_OBJ_INIT(k_msgq, msgq);
	k_object_init(msgq);

#ifdef CONFIG_POLL
	sys_dlist_init(&msgq->poll_events);
#endif /* CONFIG_POLL */

#ifdef CONFIG_OBJ_CORE_MSGQ
	k_obj_core_init_and_link(K_OBJ_CORE(msgq), &obj_type_msgq);
#endif /* CONFIG_OBJ_CORE_MSGQ */
}

/* -----------------------------------------------------------------------
 * z_impl_k_msgq_alloc_init
 * ----------------------------------------------------------------------- */

int z_impl_k_msgq_alloc_init(struct k_msgq *msgq, size_t msg_size,
			      uint32_t max_msgs)
{
	size_t total_size;
	char *buffer;

	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_msgq, alloc_init, msgq);

	if (size_mul_overflow(msg_size, max_msgs, &total_size)) {
		SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_msgq, alloc_init, msgq, -EINVAL);
		return -EINVAL;
	}

	buffer = z_thread_malloc(total_size);
	if (buffer != NULL) {
		k_msgq_init(msgq, buffer, msg_size, max_msgs);
		msgq->flags = K_MSGQ_FLAG_ALLOC;
		SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_msgq, alloc_init, msgq, 0);
		return 0;
	}

	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_msgq, alloc_init, msgq, -ENOMEM);
	return -ENOMEM;
}

#ifdef CONFIG_USERSPACE
static inline int z_vrfy_k_msgq_alloc_init(struct k_msgq *msgq,
					    size_t msg_size,
					    uint32_t max_msgs)
{
	K_OOPS(K_SYSCALL_OBJ_NEVER_INIT(msgq, K_OBJ_MSGQ));

	return z_impl_k_msgq_alloc_init(msgq, msg_size, max_msgs);
}
#include <zephyr/syscalls/k_msgq_alloc_init_mrsh.c>
#endif /* CONFIG_USERSPACE */

/* -----------------------------------------------------------------------
 * k_msgq_cleanup
 * ----------------------------------------------------------------------- */

int k_msgq_cleanup(struct k_msgq *msgq)
{
	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_msgq, cleanup, msgq);

	CHECKIF(z_waitq_head(&msgq->wait_q) != NULL) {
		SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_msgq, cleanup, msgq, -EBUSY);
		return -EBUSY;
	}

	if ((msgq->flags & K_MSGQ_FLAG_ALLOC) != 0U) {
		k_free(msgq->buffer_start);
		msgq->flags &= ~K_MSGQ_FLAG_ALLOC;
	}

	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_msgq, cleanup, msgq, 0);
	return 0;
}

/* -----------------------------------------------------------------------
 * put_msg_in_queue  (replaces msg_q.c:130-228)
 *
 * Core logic: if space available, delegate index math to verified Rust.
 * ----------------------------------------------------------------------- */

static inline int put_msg_in_queue(struct k_msgq *msgq, const void *data,
				   k_timeout_t timeout, bool put_at_back)
{
	__ASSERT(!arch_is_in_isr() || K_TIMEOUT_EQ(timeout, K_NO_WAIT), "");

	k_spinlock_key_t key = k_spin_lock(&msgq->lock);
	int result;
	bool resched = false;

	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_msgq, put, msgq, timeout);

	if (msgq->used_msgs < msgq->max_msgs) {
		/* Queue has space. */
		struct k_thread *pending_thread;

		pending_thread = z_unpend_first_thread(&msgq->wait_q);

		if (pending_thread != NULL) {
			/* A receiver was waiting — copy directly to it. */
			(void)memcpy(pending_thread->base.swap_data,
				     data, msgq->msg_size);
			arch_thread_return_value_set(pending_thread, 0);
			z_ready_thread(pending_thread);
			resched = true;
		} else if (put_at_back) {
			/* Put at back: memcpy to current write slot,
			 * then advance write index via verified Rust.
			 */
			(void)memcpy(msgq->write_ptr, data, msgq->msg_size);

			uint32_t new_write_idx, new_used;
			uint32_t cur_write = ptr_to_slot(msgq, msgq->write_ptr);

			int rc = gale_msgq_put(cur_write,
					       msgq->used_msgs,
					       msgq->max_msgs,
					       &new_write_idx,
					       &new_used);
			__ASSERT(rc == 0, "gale_msgq_put: %d", rc);
			ARG_UNUSED(rc);

			msgq->write_ptr = slot_to_ptr(msgq, new_write_idx);
			msgq->used_msgs = new_used;

#ifdef CONFIG_POLL
			resched = z_handle_obj_poll_events(
				&msgq->poll_events, K_POLL_STATE_MSGQ_DATA_AVAILABLE);
#endif
		} else {
			/* Put at front: retreat read index via verified Rust,
			 * then memcpy to new read slot.
			 */
			uint32_t new_read_idx, new_used;
			uint32_t cur_read = ptr_to_slot(msgq, msgq->read_ptr);

			int rc = gale_msgq_put_front(cur_read,
						     msgq->used_msgs,
						     msgq->max_msgs,
						     &new_read_idx,
						     &new_used);
			__ASSERT(rc == 0, "gale_msgq_put_front: %d", rc);
			ARG_UNUSED(rc);

			msgq->read_ptr = slot_to_ptr(msgq, new_read_idx);
			msgq->used_msgs = new_used;

			(void)memcpy(msgq->read_ptr, data, msgq->msg_size);

#ifdef CONFIG_POLL
			resched = z_handle_obj_poll_events(
				&msgq->poll_events, K_POLL_STATE_MSGQ_DATA_AVAILABLE);
#endif
		}

		result = 0;
	} else if (K_TIMEOUT_EQ(timeout, K_NO_WAIT)) {
		/* Queue full, non-blocking. */
		SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_msgq, put, msgq, -ENOMSG);
		result = -ENOMSG;
	} else {
		/* Queue full, blocking — pend current thread. */
		SYS_PORT_TRACING_OBJ_FUNC_BLOCKING(k_msgq, put, msgq, timeout);

		_current->base.swap_data = (void *)data;
		result = z_pend_curr(&msgq->lock, key,
				     &msgq->wait_q, timeout);

		SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_msgq, put, msgq, result);
		return result;
	}

	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_msgq, put, msgq, result);

	if (resched) {
		z_reschedule(&msgq->lock, key);
	} else {
		k_spin_unlock(&msgq->lock, key);
	}

	return result;
}

/* -----------------------------------------------------------------------
 * Public put APIs
 * ----------------------------------------------------------------------- */

int z_impl_k_msgq_put(struct k_msgq *msgq, const void *data,
		       k_timeout_t timeout)
{
	return put_msg_in_queue(msgq, data, timeout, true);
}

#ifdef CONFIG_USERSPACE
static inline int z_vrfy_k_msgq_put(struct k_msgq *msgq, const void *data,
				     k_timeout_t timeout)
{
	K_OOPS(K_SYSCALL_OBJ(msgq, K_OBJ_MSGQ));
	K_OOPS(K_SYSCALL_MEMORY_READ(data, msgq->msg_size));

	return z_impl_k_msgq_put(msgq, data, timeout);
}
#include <zephyr/syscalls/k_msgq_put_mrsh.c>
#endif /* CONFIG_USERSPACE */

int z_impl_k_msgq_put_front(struct k_msgq *msgq, const void *data)
{
	return put_msg_in_queue(msgq, data, K_NO_WAIT, false);
}

#ifdef CONFIG_USERSPACE
static inline int z_vrfy_k_msgq_put_front(struct k_msgq *msgq,
					   const void *data)
{
	K_OOPS(K_SYSCALL_OBJ(msgq, K_OBJ_MSGQ));
	K_OOPS(K_SYSCALL_MEMORY_READ(data, msgq->msg_size));

	return z_impl_k_msgq_put_front(msgq, data);
}
#include <zephyr/syscalls/k_msgq_put_front_mrsh.c>
#endif /* CONFIG_USERSPACE */

/* -----------------------------------------------------------------------
 * k_msgq_get  (replaces msg_q.c:280-349)
 * ----------------------------------------------------------------------- */

int z_impl_k_msgq_get(struct k_msgq *msgq, void *data,
		       k_timeout_t timeout)
{
	__ASSERT(!arch_is_in_isr() || K_TIMEOUT_EQ(timeout, K_NO_WAIT), "");

	k_spinlock_key_t key = k_spin_lock(&msgq->lock);
	int result;
	bool resched = false;

	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_msgq, get, msgq, timeout);

	if (msgq->used_msgs > 0U) {
		/* Queue has messages — copy from read slot. */
		(void)memcpy(data, msgq->read_ptr, msgq->msg_size);

		/* Advance read index via verified Rust. */
		uint32_t new_read_idx, new_used;
		uint32_t cur_read = ptr_to_slot(msgq, msgq->read_ptr);

		int rc = gale_msgq_get(cur_read,
				       msgq->used_msgs,
				       msgq->max_msgs,
				       &new_read_idx,
				       &new_used);
		__ASSERT(rc == 0, "gale_msgq_get: %d", rc);
		ARG_UNUSED(rc);

		msgq->read_ptr = slot_to_ptr(msgq, new_read_idx);
		msgq->used_msgs = new_used;

		/* Check for pending sender. */
		struct k_thread *pending_thread;

		pending_thread = z_unpend_first_thread(&msgq->wait_q);
		if (pending_thread != NULL) {
			/* A sender was waiting — copy its message into the
			 * queue at the current write slot.
			 */
			(void)memcpy(msgq->write_ptr,
				     (char *)pending_thread->base.swap_data,
				     msgq->msg_size);

			/* Advance write index via verified Rust. */
			uint32_t new_write_idx, new_used2;
			uint32_t cur_write = ptr_to_slot(msgq,
							 msgq->write_ptr);

			rc = gale_msgq_put(cur_write,
					   msgq->used_msgs,
					   msgq->max_msgs,
					   &new_write_idx,
					   &new_used2);
			__ASSERT(rc == 0, "gale_msgq_put (sender): %d", rc);

			msgq->write_ptr = slot_to_ptr(msgq, new_write_idx);
			msgq->used_msgs = new_used2;

			arch_thread_return_value_set(pending_thread, 0);
			z_ready_thread(pending_thread);
			resched = true;
		}

		result = 0;
	} else if (K_TIMEOUT_EQ(timeout, K_NO_WAIT)) {
		/* Queue empty, non-blocking. */
		SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_msgq, get, msgq, -ENOMSG);
		result = -ENOMSG;
	} else {
		/* Queue empty, blocking — pend current thread. */
		SYS_PORT_TRACING_OBJ_FUNC_BLOCKING(k_msgq, get, msgq, timeout);

		_current->base.swap_data = data;
		result = z_pend_curr(&msgq->lock, key,
				     &msgq->wait_q, timeout);

		SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_msgq, get, msgq, result);
		return result;
	}

	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_msgq, get, msgq, result);

	if (resched) {
		z_reschedule(&msgq->lock, key);
	} else {
		k_spin_unlock(&msgq->lock, key);
	}

	return result;
}

#ifdef CONFIG_USERSPACE
static inline int z_vrfy_k_msgq_get(struct k_msgq *msgq, void *data,
				     k_timeout_t timeout)
{
	K_OOPS(K_SYSCALL_OBJ(msgq, K_OBJ_MSGQ));
	K_OOPS(K_SYSCALL_MEMORY_WRITE(data, msgq->msg_size));

	return z_impl_k_msgq_get(msgq, data, timeout);
}
#include <zephyr/syscalls/k_msgq_get_mrsh.c>
#endif /* CONFIG_USERSPACE */

/* -----------------------------------------------------------------------
 * k_msgq_peek / k_msgq_peek_at
 * ----------------------------------------------------------------------- */

int z_impl_k_msgq_peek(struct k_msgq *msgq, void *data)
{
	k_spinlock_key_t key = k_spin_lock(&msgq->lock);
	int result;

	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_msgq, peek, msgq);

	if (msgq->used_msgs > 0U) {
		(void)memcpy(data, msgq->read_ptr, msgq->msg_size);
		result = 0;
	} else {
		result = -ENOMSG;
	}

	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_msgq, peek, msgq, result);
	k_spin_unlock(&msgq->lock, key);

	return result;
}

#ifdef CONFIG_USERSPACE
static inline int z_vrfy_k_msgq_peek(struct k_msgq *msgq, void *data)
{
	K_OOPS(K_SYSCALL_OBJ(msgq, K_OBJ_MSGQ));
	K_OOPS(K_SYSCALL_MEMORY_WRITE(data, msgq->msg_size));

	return z_impl_k_msgq_peek(msgq, data);
}
#include <zephyr/syscalls/k_msgq_peek_mrsh.c>
#endif /* CONFIG_USERSPACE */

int z_impl_k_msgq_peek_at(struct k_msgq *msgq, void *data, uint32_t idx)
{
	k_spinlock_key_t key = k_spin_lock(&msgq->lock);
	int result;

	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_msgq, peek_at, msgq);

	uint32_t slot;

	int rc = gale_msgq_peek_at(ptr_to_slot(msgq, msgq->read_ptr),
				   msgq->used_msgs,
				   msgq->max_msgs,
				   idx,
				   &slot);

	if (rc == 0) {
		(void)memcpy(data, slot_to_ptr(msgq, slot), msgq->msg_size);
		result = 0;
	} else {
		result = -ENOMSG;
	}

	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_msgq, peek_at, msgq, result);
	k_spin_unlock(&msgq->lock, key);

	return result;
}

#ifdef CONFIG_USERSPACE
static inline int z_vrfy_k_msgq_peek_at(struct k_msgq *msgq,
					 void *data, uint32_t idx)
{
	K_OOPS(K_SYSCALL_OBJ(msgq, K_OBJ_MSGQ));
	K_OOPS(K_SYSCALL_MEMORY_WRITE(data, msgq->msg_size));

	return z_impl_k_msgq_peek_at(msgq, data, idx);
}
#include <zephyr/syscalls/k_msgq_peek_at_mrsh.c>
#endif /* CONFIG_USERSPACE */

/* -----------------------------------------------------------------------
 * k_msgq_purge  (replaces msg_q.c:443-470)
 * ----------------------------------------------------------------------- */

void z_impl_k_msgq_purge(struct k_msgq *msgq)
{
	k_spinlock_key_t key = k_spin_lock(&msgq->lock);
	struct k_thread *pending_thread;
	bool resched = false;

	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_msgq, purge, msgq);

	/* Wake all pending threads with -ENOMSG. */
	while ((pending_thread = z_unpend_first_thread(&msgq->wait_q))
	       != NULL) {
		arch_thread_return_value_set(pending_thread, -ENOMSG);
		z_ready_thread(pending_thread);
		resched = true;
	}

	/* Reset indices — read_ptr = write_ptr, used_msgs = 0. */
	msgq->used_msgs = 0U;
	msgq->read_ptr = msgq->write_ptr;

	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_msgq, purge, msgq);

	if (resched) {
		z_reschedule(&msgq->lock, key);
	} else {
		k_spin_unlock(&msgq->lock, key);
	}
}

#ifdef CONFIG_USERSPACE
static inline void z_vrfy_k_msgq_purge(struct k_msgq *msgq)
{
	K_OOPS(K_SYSCALL_OBJ(msgq, K_OBJ_MSGQ));

	z_impl_k_msgq_purge(msgq);
}
#include <zephyr/syscalls/k_msgq_purge_mrsh.c>
#endif /* CONFIG_USERSPACE */

/* -----------------------------------------------------------------------
 * k_msgq_get_attrs
 * ----------------------------------------------------------------------- */

void z_impl_k_msgq_get_attrs(struct k_msgq *msgq,
			      struct k_msgq_attrs *attrs)
{
	attrs->msg_size = msgq->msg_size;
	attrs->max_msgs = msgq->max_msgs;
	attrs->used_msgs = msgq->used_msgs;
}

#ifdef CONFIG_USERSPACE
static inline void z_vrfy_k_msgq_get_attrs(struct k_msgq *msgq,
					    struct k_msgq_attrs *attrs)
{
	K_OOPS(K_SYSCALL_OBJ(msgq, K_OBJ_MSGQ));
	K_OOPS(K_SYSCALL_MEMORY_WRITE(attrs, sizeof(struct k_msgq_attrs)));

	z_impl_k_msgq_get_attrs(msgq, attrs);
}
#include <zephyr/syscalls/k_msgq_get_attrs_mrsh.c>
#endif /* CONFIG_USERSPACE */
