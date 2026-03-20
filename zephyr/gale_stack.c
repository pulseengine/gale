/*
 * Copyright (c) 2010-2016 Wind River Systems, Inc.
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale stack — verified LIFO count/capacity arithmetic.
 *
 * This is kernel/stack.c with push/pop rewritten to use Rust decision
 * structs.  C extracts kernel state (spinlock, wait queue peek),
 * Rust decides the action, C applies it.
 *
 * Verified operations (Verus proofs):
 *   gale_k_stack_push_decide — SK1 (bounds), SK3 (increment), SK4 (-ENOMEM)
 *   gale_k_stack_pop_decide  — SK1 (bounds), SK5 (decrement), SK6 (-EBUSY)
 *   gale_stack_init_validate — SK2 (capacity > 0)
 */

#include <zephyr/kernel.h>
#include <zephyr/kernel_structs.h>

#include <zephyr/toolchain.h>
#include <zephyr/linker/sections.h>
#include <wait_q.h>
#include <ksched.h>
#include <zephyr/sys/check.h>
#include <zephyr/init.h>
#include <zephyr/internal/syscall_handler.h>
#include <zephyr/sys/math_extras.h>

#include "gale_stack.h"

#ifdef CONFIG_OBJ_CORE_STACK
static struct k_obj_type obj_type_stack;
#endif /* CONFIG_OBJ_CORE_STACK */

void k_stack_init(struct k_stack *stack, stack_data_t *buffer,
		  uint32_t num_entries)
{
	z_waitq_init(&stack->wait_q);
	stack->lock = (struct k_spinlock) {};
	stack->next = buffer;
	stack->base = buffer;
	stack->top = stack->base + num_entries;

	SYS_PORT_TRACING_OBJ_INIT(k_stack, stack);
	k_object_init(stack);

#ifdef CONFIG_OBJ_CORE_STACK
	k_obj_core_init_and_link(K_OBJ_CORE(stack), &obj_type_stack);
#endif /* CONFIG_OBJ_CORE_STACK */
}

int32_t z_impl_k_stack_alloc_init(struct k_stack *stack, uint32_t num_entries)
{
	void *buffer;
	int32_t ret;

	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_stack, alloc_init, stack);

	buffer = z_thread_malloc(num_entries * sizeof(stack_data_t));
	if (buffer != NULL) {
		k_stack_init(stack, buffer, num_entries);
		stack->flags = K_STACK_FLAG_ALLOC;
		ret = 0;
	} else {
		ret = -ENOMEM;
	}

	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_stack, alloc_init, stack, ret);

	return ret;
}

#ifdef CONFIG_USERSPACE
static inline int32_t z_vrfy_k_stack_alloc_init(struct k_stack *stack,
					      uint32_t num_entries)
{
	size_t total_size;

	K_OOPS(K_SYSCALL_OBJ_NEVER_INIT(stack, K_OBJ_STACK));
	K_OOPS(K_SYSCALL_VERIFY(num_entries > 0));
	K_OOPS(K_SYSCALL_VERIFY(!size_mul_overflow(num_entries, sizeof(stack_data_t),
					&total_size)));
	return z_impl_k_stack_alloc_init(stack, num_entries);
}
#include <zephyr/syscalls/k_stack_alloc_init_mrsh.c>
#endif /* CONFIG_USERSPACE */

int k_stack_cleanup(struct k_stack *stack)
{
	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_stack, cleanup, stack);

	CHECKIF(z_waitq_head(&stack->wait_q) != NULL) {
		SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_stack, cleanup, stack,
						-EAGAIN);
		return -EAGAIN;
	}

	if ((stack->flags & K_STACK_FLAG_ALLOC) != (uint8_t)0) {
		k_free(stack->base);
		stack->base = NULL;
		stack->flags &= ~K_STACK_FLAG_ALLOC;
	}

	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_stack, cleanup, stack, 0);

	return 0;
}

int z_impl_k_stack_push(struct k_stack *stack, stack_data_t data)
{
	k_spinlock_key_t key = k_spin_lock(&stack->lock);

	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_stack, push, stack);

	/* Extract: read state, peek at wait queue head (no side effect) */
	uint32_t count = (uint32_t)(stack->next - stack->base);
	uint32_t capacity = (uint32_t)(stack->top - stack->base);
	uint32_t has_waiter = (z_waitq_head(&stack->wait_q) != NULL) ? 1U : 0U;

	/* Decide: Rust determines action based on count, capacity, waiter */
	struct gale_stack_push_decision d = gale_k_stack_push_decide(
		count, capacity, has_waiter);

	/* Apply: execute Rust's decision */
	if (d.action == GALE_STACK_PUSH_WAKE) {
		struct k_thread *thread = z_unpend_first_thread(&stack->wait_q);

		z_thread_return_value_set_with_data(thread,
						    0, (void *)data);
		z_ready_thread(thread);
		z_reschedule(&stack->lock, key);
	} else if (d.action == GALE_STACK_PUSH_STORE) {
		/* SK3: count incremented — store data and advance pointer */
		*(stack->next) = data;
		stack->next++;
		k_spin_unlock(&stack->lock, key);
	} else {
		/* FULL: -ENOMEM */
		k_spin_unlock(&stack->lock, key);
	}

	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_stack, push, stack, d.ret);

	return d.ret;
}

#ifdef CONFIG_USERSPACE
static inline int z_vrfy_k_stack_push(struct k_stack *stack, stack_data_t data)
{
	K_OOPS(K_SYSCALL_OBJ(stack, K_OBJ_STACK));

	return z_impl_k_stack_push(stack, data);
}
#include <zephyr/syscalls/k_stack_push_mrsh.c>
#endif /* CONFIG_USERSPACE */

int z_impl_k_stack_pop(struct k_stack *stack, stack_data_t *data,
		       k_timeout_t timeout)
{
	k_spinlock_key_t key;
	int result;

	key = k_spin_lock(&stack->lock);

	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_stack, pop, stack, timeout);

	/* Extract: read count and timeout mode */
	uint32_t count = (uint32_t)(stack->next - stack->base);

	/* Decide: Rust determines pop/pend/busy */
	struct gale_stack_pop_decision d = gale_k_stack_pop_decide(
		count, K_TIMEOUT_EQ(timeout, K_NO_WAIT) ? 1U : 0U);

	/* Apply: execute Rust's decision */
	if (d.action == GALE_STACK_POP_OK) {
		if (d.ret == 0) {
			/* SK5: count decremented — read data and retreat pointer */
			stack->next--;
			*data = *(stack->next);
		}
		k_spin_unlock(&stack->lock, key);

		SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_stack, pop, stack,
						timeout, d.ret);
		return d.ret;
	}

	/* PEND_CURRENT: block on wait queue with timeout */
	SYS_PORT_TRACING_OBJ_FUNC_BLOCKING(k_stack, pop, stack, timeout);

	result = z_pend_curr(&stack->lock, key, &stack->wait_q, timeout);
	if (result == -EAGAIN) {
		SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_stack, pop, stack,
						timeout, -EAGAIN);
		return -EAGAIN;
	}

	*data = (stack_data_t)_current->base.swap_data;

	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_stack, pop, stack, timeout, 0);

	return 0;
}

#ifdef CONFIG_USERSPACE
static inline int z_vrfy_k_stack_pop(struct k_stack *stack,
				     stack_data_t *data, k_timeout_t timeout)
{
	K_OOPS(K_SYSCALL_OBJ(stack, K_OBJ_STACK));
	K_OOPS(K_SYSCALL_MEMORY_WRITE(data, sizeof(stack_data_t)));
	return z_impl_k_stack_pop(stack, data, timeout);
}
#include <zephyr/syscalls/k_stack_pop_mrsh.c>
#endif /* CONFIG_USERSPACE */

#ifdef CONFIG_OBJ_CORE_STACK
static int init_stack_obj_core_list(void)
{
	/* Initialize stack object type */

	z_obj_type_init(&obj_type_stack, K_OBJ_TYPE_STACK_ID,
			offsetof(struct k_stack, obj_core));

	/* Initialize and link statically defined stacks */

	STRUCT_SECTION_FOREACH(k_stack, stack) {
		k_obj_core_init_and_link(K_OBJ_CORE(stack), &obj_type_stack);
	}

	return 0;
}

SYS_INIT(init_stack_obj_core_list, PRE_KERNEL_1,
	 CONFIG_KERNEL_INIT_PRIORITY_OBJECTS);
#endif /* CONFIG_OBJ_CORE_STACK */
