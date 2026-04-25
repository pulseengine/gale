/*
 * Gale Pipe FFI — verified state machine + byte count validation.
 *
 * These functions replace the state checks and byte count computation
 * in kernel/pipe.c.  The actual ring buffer (indices, memcpy) stays
 * in Zephyr's ring_buf subsystem.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#ifndef GALE_PIPE_H
#define GALE_PIPE_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Validate a pipe write and compute how many bytes can be written.
 *
 * Caller does ring_buf_put() with *actual_len bytes after this succeeds.
 *
 * @param used        Current bytes in buffer (ring_buf_size_get).
 * @param size        Buffer capacity.
 * @param flags       Pipe flags (PIPE_FLAG_OPEN, PIPE_FLAG_RESET).
 * @param request_len Bytes the caller wants to write.
 * @param actual_len  Output: actual bytes to write (min of request and free).
 * @param new_used    Output: updated used count after write.
 *
 * @return 0 on success, -EPIPE (closed), -ECANCELED (resetting),
 *         -EAGAIN (full), -ENOMSG (zero request).
 */
int32_t gale_pipe_write_check(uint32_t used,
                               uint32_t size,
                               uint8_t flags,
                               uint32_t request_len,
                               uint32_t *actual_len,
                               uint32_t *new_used);

/**
 * Validate a pipe read and compute how many bytes can be read.
 *
 * Caller does ring_buf_get() with *actual_len bytes after this succeeds.
 *
 * @param used        Current bytes in buffer.
 * @param flags       Pipe flags.
 * @param request_len Bytes the caller wants to read.
 * @param actual_len  Output: actual bytes to read (min of request and used).
 * @param new_used    Output: updated used count after read.
 *
 * @return 0 on success, -EPIPE (closed+empty), -ECANCELED (resetting),
 *         -EAGAIN (empty), -ENOMSG (zero request).
 */
int32_t gale_pipe_read_check(uint32_t used,
                              uint8_t flags,
                              uint32_t request_len,
                              uint32_t *actual_len,
                              uint32_t *new_used);

/* ---- Phase 2: Full Decision API ----
 *
 * Redesigned 2026-04-25 from 16-byte structs to 8 bytes so the FFI
 * returns via uint64_t (AAPCS r0/r1 register pair) instead of sret —
 * required for the LLVM cross-language inliner (see gale issue #10).
 *
 *   - new_used dropped (caller computes new_used = old_used +/- actual_bytes,
 *     verified unused by the C consumer prior to redesign)
 *   - ret dropped (caller derives error code from the action variant)
 *   - WRITE_ERROR / READ_ERROR split into ECANCELED + EPIPE variants
 *     to preserve the -EPIPE distinction the read consumer relies on.
 */

struct gale_pipe_write_decision {
    uint8_t action;      /* see GALE_PIPE_ACTION_WRITE_* below */
    uint32_t actual_bytes;
};

#define GALE_PIPE_ACTION_WRITE_OK                0
#define GALE_PIPE_ACTION_WAKE_READER             1
#define GALE_PIPE_ACTION_WRITE_PEND              2
#define GALE_PIPE_ACTION_WRITE_ERROR_ECANCELED   3
#define GALE_PIPE_ACTION_WRITE_ERROR_EPIPE       4

uint64_t gale_k_pipe_write_decide(
    uint32_t used, uint32_t size, uint8_t flags,
    uint32_t request_len, uint32_t has_reader);

union gale_pipe_write_decision_u {
    uint64_t raw;
    struct gale_pipe_write_decision dec;
};

struct gale_pipe_read_decision {
    uint8_t action;      /* see GALE_PIPE_ACTION_READ_* below */
    uint32_t actual_bytes;
};

#define GALE_PIPE_ACTION_READ_OK                 0
#define GALE_PIPE_ACTION_WAKE_WRITER             1
#define GALE_PIPE_ACTION_READ_PEND               2
#define GALE_PIPE_ACTION_READ_ERROR_ECANCELED    3
#define GALE_PIPE_ACTION_READ_ERROR_EPIPE        4

uint64_t gale_k_pipe_read_decide(
    uint32_t used, uint32_t size, uint8_t flags,
    uint32_t request_len, uint32_t has_writer);

union gale_pipe_read_decision_u {
    uint64_t raw;
    struct gale_pipe_read_decision dec;
};

#ifdef __cplusplus
}
#endif

#endif /* GALE_PIPE_H */
