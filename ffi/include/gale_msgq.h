/*
 * Gale Message Queue FFI — verified ring buffer index arithmetic.
 *
 * These functions replace the index computation in kernel/msg_q.c.
 * The C shim converts slot indices to byte pointers:
 *   byte_ptr = buffer_start + slot_idx * msg_size
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#ifndef GALE_MSGQ_H
#define GALE_MSGQ_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Validate message queue init parameters.
 *
 * @param msg_size    Size of each message in bytes.
 * @param max_msgs    Maximum number of messages.
 * @param buffer_size Output: msg_size * max_msgs (buffer allocation size).
 *
 * @return 0 on success, -EINVAL if msg_size==0, max_msgs==0, or overflow.
 */
int32_t gale_msgq_init_validate(uint32_t msg_size,
                                uint32_t max_msgs,
                                uint32_t *buffer_size);

/**
 * Compute new write index after putting a message at the back.
 *
 * Caller does memcpy at: buffer_start + write_idx * msg_size
 * before calling this (the slot to write is the current write_idx).
 *
 * @param write_idx     Current write slot index.
 * @param used_msgs     Current message count.
 * @param max_msgs      Queue capacity.
 * @param new_write_idx Output: advanced write index.
 * @param new_used      Output: incremented used count.
 *
 * @return 0 on success, -ENOMSG if queue full.
 */
int32_t gale_msgq_put(uint32_t write_idx,
                      uint32_t used_msgs,
                      uint32_t max_msgs,
                      uint32_t *new_write_idx,
                      uint32_t *new_used);

/**
 * Compute new read index after putting a message at the front.
 *
 * Caller does memcpy at: buffer_start + *new_read_idx * msg_size
 * after calling this.
 *
 * @param read_idx     Current read slot index.
 * @param used_msgs    Current message count.
 * @param max_msgs     Queue capacity.
 * @param new_read_idx Output: retreated read index (write target).
 * @param new_used     Output: incremented used count.
 *
 * @return 0 on success, -ENOMSG if queue full.
 */
int32_t gale_msgq_put_front(uint32_t read_idx,
                            uint32_t used_msgs,
                            uint32_t max_msgs,
                            uint32_t *new_read_idx,
                            uint32_t *new_used);

/**
 * Compute new read index after getting a message.
 *
 * Caller does memcpy from: buffer_start + read_idx * msg_size
 * before calling this (the slot to read is the current read_idx).
 *
 * @param read_idx     Current read slot index.
 * @param used_msgs    Current message count.
 * @param max_msgs     Queue capacity.
 * @param new_read_idx Output: advanced read index.
 * @param new_used     Output: decremented used count.
 *
 * @return 0 on success, -ENOMSG if queue empty.
 */
int32_t gale_msgq_get(uint32_t read_idx,
                      uint32_t used_msgs,
                      uint32_t max_msgs,
                      uint32_t *new_read_idx,
                      uint32_t *new_used);

/**
 * Compute the buffer slot index for peeking at a message.
 *
 * Caller does memcpy from: buffer_start + *slot_idx * msg_size
 *
 * @param read_idx  Current read slot index.
 * @param used_msgs Current message count.
 * @param max_msgs  Queue capacity.
 * @param idx       Message index (0 = first/oldest).
 * @param slot_idx  Output: computed slot index.
 *
 * @return 0 on success, -ENOMSG if idx >= used_msgs.
 */
int32_t gale_msgq_peek_at(uint32_t read_idx,
                          uint32_t used_msgs,
                          uint32_t max_msgs,
                          uint32_t idx,
                          uint32_t *slot_idx);

#ifdef __cplusplus
}
#endif

#endif /* GALE_MSGQ_H */
