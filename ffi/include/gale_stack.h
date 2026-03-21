/*
 * Gale Stack FFI — verified LIFO count/capacity arithmetic.
 *
 * These functions replace the capacity check and count tracking
 * in kernel/stack.c.  The C shim converts between pointer
 * differences and count/capacity values:
 *   count    = (uint32_t)(stack->next - stack->base)
 *   capacity = (uint32_t)(stack->top  - stack->base)
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#ifndef GALE_STACK_H
#define GALE_STACK_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Validate stack init parameters.
 *
 * @param num_entries  Number of stack entries (capacity).
 *
 * @return 0 on success, -EINVAL if num_entries == 0.
 */
int32_t gale_stack_init_validate(uint32_t num_entries);

/**
 * Validate a push operation and compute new count.
 *
 * Caller stores data at stack->next before advancing:
 *   *(stack->next) = data; stack->next++;
 *
 * @param count     Current element count (next - base).
 * @param capacity  Maximum entries (top - base).
 * @param new_count Output: count + 1.
 *
 * @return 0 on success, -ENOMEM if stack full.
 */
int32_t gale_stack_push_validate(uint32_t count,
                                  uint32_t capacity,
                                  uint32_t *new_count);

/**
 * Validate a pop operation and compute new count.
 *
 * Caller reads data after decrementing:
 *   stack->next--; *data = *(stack->next);
 *
 * @param count     Current element count (next - base).
 * @param new_count Output: count - 1.
 *
 * @return 0 on success, -EBUSY if stack empty.
 */
int32_t gale_stack_pop_validate(uint32_t count,
                                 uint32_t *new_count);

/* ---- Phase 2: Full Decision API ---- */

struct gale_stack_push_decision {
    int32_t ret;
    uint32_t new_count;
    uint8_t action;     /* 0=STORE_DATA, 1=WAKE_WAITER, 2=FULL */
};

#define GALE_STACK_PUSH_STORE 0
#define GALE_STACK_PUSH_WAKE  1
#define GALE_STACK_PUSH_FULL  2

struct gale_stack_push_decision gale_k_stack_push_decide(
    uint32_t count, uint32_t capacity, uint32_t has_waiter);

struct gale_stack_pop_decision {
    int32_t ret;
    uint32_t new_count;
    uint8_t action;     /* 0=POP_OK, 1=PEND_CURRENT */
};

#define GALE_STACK_POP_OK   0
#define GALE_STACK_POP_PEND 1

struct gale_stack_pop_decision gale_k_stack_pop_decide(
    uint32_t count, uint32_t is_no_wait);

#ifdef __cplusplus
}
#endif

#endif /* GALE_STACK_H */
