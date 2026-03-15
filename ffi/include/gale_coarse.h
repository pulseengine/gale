/*
 * Gale Coarsened FFI — struct-based state passing (v2).
 *
 * These functions replace individual scalar parameters with #[repr(C)]
 * state structs, improving type safety and reducing parameter count.
 * The v1 scalar API is unchanged; C shims can migrate incrementally.
 *
 * Scope: semaphore, stack, pipe (proof of concept).
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#ifndef GALE_COARSE_H
#define GALE_COARSE_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* -----------------------------------------------------------------------
 * State structs
 * ----------------------------------------------------------------------- */

/** Semaphore state: count + limit. */
struct gale_sem_state {
    uint32_t count;
    uint32_t limit;
};

/** Stack state: count + capacity. */
struct gale_stack_state {
    uint32_t count;
    uint32_t capacity;
};

/** Pipe state: used bytes, buffer capacity, flags. */
struct gale_pipe_state {
    uint32_t used;
    uint32_t size;
    uint8_t  flags;
};

/* -----------------------------------------------------------------------
 * Semaphore v2
 * ----------------------------------------------------------------------- */

/**
 * Validate semaphore init parameters (v2, struct-based).
 *
 * @param state  Pointer to semaphore state (read-only).
 *
 * @return 0 on success, -EINVAL if null, limit == 0, or count > limit.
 */
int32_t gale_sem_validate_v2(const struct gale_sem_state *state);

/**
 * Give (signal) a semaphore: increment count up to limit (v2).
 *
 * On success, state->count is updated in place.
 *
 * @param state  Pointer to semaphore state (read-write).
 *
 * @return 0 on success, -EINVAL if null.
 */
int32_t gale_sem_give_v2(struct gale_sem_state *state);

/**
 * Take (acquire) a semaphore: decrement count if > 0 (v2).
 *
 * On success, state->count is decremented in place.
 *
 * @param state  Pointer to semaphore state (read-write).
 *
 * @return 0 on success, -EBUSY if count == 0, -EINVAL if null.
 */
int32_t gale_sem_take_v2(struct gale_sem_state *state);

/* -----------------------------------------------------------------------
 * Stack v2
 * ----------------------------------------------------------------------- */

/**
 * Validate stack init parameters (v2, struct-based).
 *
 * @param state  Pointer to stack state (read-only).
 *
 * @return 0 on success, -EINVAL if null or capacity == 0.
 */
int32_t gale_stack_init_validate_v2(const struct gale_stack_state *state);

/**
 * Push onto stack: increment count if below capacity (v2).
 *
 * On success, state->count is incremented in place.
 *
 * @param state  Pointer to stack state (read-write).
 *
 * @return 0 on success, -ENOMEM if full, -EINVAL if null.
 */
int32_t gale_stack_push_v2(struct gale_stack_state *state);

/**
 * Pop from stack: decrement count if > 0 (v2).
 *
 * On success, state->count is decremented in place.
 *
 * @param state  Pointer to stack state (read-write).
 *
 * @return 0 on success, -EBUSY if empty, -EINVAL if null.
 */
int32_t gale_stack_pop_v2(struct gale_stack_state *state);

/* -----------------------------------------------------------------------
 * Pipe v2
 * ----------------------------------------------------------------------- */

/**
 * Validate a pipe write and compute how many bytes can be written (v2).
 *
 * On success, state->used is updated to the new byte count.
 *
 * @param state       Pointer to pipe state (read-write).
 * @param request_len Bytes the caller wants to write.
 * @param actual_len  Output: actual bytes to write.
 *
 * @return 0 on success, -EPIPE (closed), -ECANCELED (resetting),
 *         -EAGAIN (full), -ENOMSG (zero request), -EINVAL (null/zero-size).
 */
int32_t gale_pipe_write_v2(struct gale_pipe_state *state,
                            uint32_t request_len,
                            uint32_t *actual_len);

/**
 * Validate a pipe read and compute how many bytes can be read (v2).
 *
 * On success, state->used is updated to the new byte count.
 *
 * @param state       Pointer to pipe state (read-write).
 * @param request_len Bytes the caller wants to read.
 * @param actual_len  Output: actual bytes to read.
 *
 * @return 0 on success, -EPIPE (closed+empty), -ECANCELED (resetting),
 *         -EAGAIN (empty), -ENOMSG (zero request), -EINVAL (null).
 */
int32_t gale_pipe_read_v2(struct gale_pipe_state *state,
                           uint32_t request_len,
                           uint32_t *actual_len);

#ifdef __cplusplus
}
#endif

#endif /* GALE_COARSE_H */
