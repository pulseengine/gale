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

#ifdef __cplusplus
}
#endif

#endif /* GALE_PIPE_H */
