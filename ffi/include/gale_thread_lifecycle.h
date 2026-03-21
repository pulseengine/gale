/*
 * Gale Thread Lifecycle FFI — verified create/exit counting, priority
 * validation, and decision structs for create/abort/join.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#ifndef GALE_THREAD_LIFECYCLE_H
#define GALE_THREAD_LIFECYCLE_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Validate thread creation: check count < MAX_THREADS and increment.
 *
 * @param count      Current active thread count.
 * @param new_count  Output: count + 1.
 *
 * @return 0 on success, -EAGAIN at capacity, -EINVAL on null pointer.
 */
int32_t gale_thread_create_validate(uint32_t count, uint32_t *new_count);

/**
 * Validate thread exit: check count > 0 and decrement.
 *
 * @param count      Current active thread count.
 * @param new_count  Output: count - 1.
 *
 * @return 0 on success, -EINVAL on underflow or null pointer.
 */
int32_t gale_thread_exit_validate(uint32_t count, uint32_t *new_count);

/**
 * Validate a thread priority value.
 *
 * @param priority  Proposed priority value.
 *
 * @return 0 if valid (< MAX_PRIORITY), -EINVAL if out of range.
 */
int32_t gale_thread_priority_validate(uint32_t priority);

/* ---- Phase 2: Full Decision API ---- */

/* -- Thread Create Decision -- */

struct gale_thread_create_decision {
    uint8_t action;     /* 0=PROCEED, 1=REJECT */
    int32_t ret;        /* 0 (OK) or negative errno */
};

#define GALE_THREAD_ACTION_PROCEED 0
#define GALE_THREAD_ACTION_REJECT  1

/**
 * Decide whether to proceed with thread creation.
 *
 * Validates stack_size, priority, options, and active thread count.
 * All arch-specific init, TLS, naming stay in C.
 *
 * @param stack_size    Proposed stack size in bytes.
 * @param priority      Proposed thread priority.
 * @param options       Thread creation options (K_ESSENTIAL, K_USER, etc.).
 * @param active_count  Current active thread count.
 *
 * @return Decision struct: action=PROCEED or REJECT with errno.
 */
struct gale_thread_create_decision gale_k_thread_create_decide(
    uint32_t stack_size, uint32_t priority, uint32_t options,
    uint32_t active_count);

/* -- Thread Abort Decision -- */

struct gale_thread_abort_decision {
    uint8_t action;     /* 0=ABORT, 1=ALREADY_DEAD, 2=PANIC */
};

#define GALE_THREAD_ABORT_PROCEED      0
#define GALE_THREAD_ABORT_ALREADY_DEAD 1
#define GALE_THREAD_ABORT_PANIC        2

/**
 * Decide what action to take for thread abort.
 *
 * @param thread_state  thread_base.thread_state flags.
 * @param is_essential  1 if thread has K_ESSENTIAL flag, 0 otherwise.
 *
 * @return Decision struct: action=ABORT, ALREADY_DEAD, or PANIC.
 */
struct gale_thread_abort_decision gale_k_thread_abort_decide(
    uint8_t thread_state, uint32_t is_essential);

/* -- Thread Join Decision -- */

struct gale_thread_join_decision {
    uint8_t action;     /* 0=RETURN_IMMEDIATELY, 1=PEND_ON_JOIN_QUEUE */
    int32_t ret;        /* 0 (OK), -EBUSY, -EDEADLK */
};

#define GALE_THREAD_JOIN_RETURN 0
#define GALE_THREAD_JOIN_PEND   1

/**
 * Decide what action to take for thread join.
 *
 * @param is_dead             1 if target thread is dead, 0 otherwise.
 * @param is_no_wait          1 if timeout == K_NO_WAIT, 0 otherwise.
 * @param is_self_or_circular 1 if joining self or circular dependency.
 *
 * @return Decision struct: action=RETURN or PEND, with ret code.
 */
struct gale_thread_join_decision gale_k_thread_join_decide(
    uint32_t is_dead, uint32_t is_no_wait, uint32_t is_self_or_circular);

#ifdef __cplusplus
}
#endif

#endif /* GALE_THREAD_LIFECYCLE_H */
