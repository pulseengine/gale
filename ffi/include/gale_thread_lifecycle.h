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

/* -- Thread Suspend Decision -- */

struct gale_thread_suspend_decision {
    uint8_t action;     /* 0=PROCEED, 1=ALREADY_SUSPENDED */
};

#define GALE_THREAD_SUSPEND_PROCEED           0
#define GALE_THREAD_SUSPEND_ALREADY_SUSPENDED 1

/**
 * Decide whether to proceed with k_thread_suspend.
 *
 * TH7: Suspending an already-suspended thread is idempotent.
 * Source: sched.c:491-522 z_impl_k_thread_suspend.
 *
 * @param thread_state  thread_base.thread_state flags.
 *
 * @return Decision: PROCEED or ALREADY_SUSPENDED.
 */
struct gale_thread_suspend_decision gale_k_thread_suspend_decide(
    uint8_t thread_state);

/* -- Thread Resume Decision -- */

struct gale_thread_resume_decision {
    uint8_t action;     /* 0=PROCEED, 1=NOT_SUSPENDED */
};

#define GALE_THREAD_RESUME_PROCEED       0
#define GALE_THREAD_RESUME_NOT_SUSPENDED 1

/**
 * Decide whether to proceed with k_thread_resume.
 *
 * TH8: Resuming a non-suspended thread is idempotent.
 * Source: sched.c:533-551 z_impl_k_thread_resume.
 *
 * @param thread_state  thread_base.thread_state flags.
 *
 * @return Decision: PROCEED or NOT_SUSPENDED.
 */
struct gale_thread_resume_decision gale_k_thread_resume_decide(
    uint8_t thread_state);

/* -- Thread Priority Set Decision -- */

struct gale_thread_priority_set_decision {
    uint8_t action;     /* 0=PROCEED, 1=REJECT */
    int32_t ret;        /* 0 (OK) or -EINVAL */
};

#define GALE_THREAD_PRIO_SET_PROCEED 0
#define GALE_THREAD_PRIO_SET_REJECT  1

/**
 * Decide whether to proceed with k_thread_priority_set.
 *
 * TH1/TH2: Priority must be in [0, MAX_PRIORITY).
 * Source: sched.c:1009-1023 z_impl_k_thread_priority_set.
 *
 * @param new_priority  Proposed priority value.
 *
 * @return Decision: PROCEED (valid) or REJECT with -EINVAL.
 */
struct gale_thread_priority_set_decision gale_k_thread_priority_set_decide(
    uint32_t new_priority);

/* -- Thread Stack Space Decision -- */

struct gale_thread_stack_space_decision {
    uint8_t  action;          /* 0=PROCEED, 1=REJECT */
    int32_t  ret;             /* 0 (OK) or -EINVAL */
    uint32_t unused_estimate; /* upper-bound unused bytes (valid when PROCEED) */
};

#define GALE_THREAD_STACK_SPACE_PROCEED 0
#define GALE_THREAD_STACK_SPACE_REJECT  1

/**
 * Decide whether k_thread_stack_space_get can proceed.
 *
 * TH4: unused_estimate <= stack_size (bounded by watermark invariant).
 * Source: thread.c:1067-1078 z_impl_k_thread_stack_space_get.
 *
 * @param stack_size         Usable stack size in bytes.
 * @param stack_usage        High-watermark usage in bytes.
 * @param stack_mapped_valid 1 if stack is accessible, 0 for invalid mapped stack.
 *
 * @return Decision: PROCEED with unused_estimate, or REJECT.
 */
struct gale_thread_stack_space_decision gale_k_thread_stack_space_decide(
    uint32_t stack_size, uint32_t stack_usage, uint32_t stack_mapped_valid);

/* -- Thread Deadline Decision -- */

struct gale_thread_deadline_decision {
    uint8_t action;           /* 0=PROCEED, 1=REJECT */
    int32_t ret;              /* 0 (OK) or -EINVAL */
    int32_t clamped_deadline; /* clamped value (== deadline for valid inputs) */
};

#define GALE_THREAD_DEADLINE_PROCEED 0
#define GALE_THREAD_DEADLINE_REJECT  1

/**
 * Decide whether a deadline value is valid for k_thread_deadline_set.
 *
 * TD1/TD3: deadline must be > 0; zero or negative values are rejected.
 * Source: sched.c:1063-1095 z_impl/z_vrfy_k_thread_deadline_set.
 *
 * @param deadline  Proposed deadline in cycles.
 *
 * @return Decision: PROCEED with clamped value, or REJECT with -EINVAL.
 */
struct gale_thread_deadline_decision gale_k_thread_deadline_decide(
    int32_t deadline);

#ifdef __cplusplus
}
#endif

#endif /* GALE_THREAD_LIFECYCLE_H */
