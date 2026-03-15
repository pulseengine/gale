/*
 * Gale Mbox FFI — verified stateless mailbox validation.
 *
 * These functions replace the message matching logic and
 * data exchange computation in kernel/mailbox.c.  Thread
 * identity matching is simplified to integer IDs where
 * 0 represents K_ANY.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#ifndef GALE_MBOX_H
#define GALE_MBOX_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Validate a mailbox send operation.
 *
 * @param size  Message data size in bytes.
 *
 * @return 0 on success, -EINVAL if size == 0.
 */
int32_t gale_mbox_validate_send(uint32_t size);

/**
 * Check if sender and receiver IDs are compatible.
 *
 * Uses integer IDs where 0 means K_ANY (match any thread).
 *
 * @param send_id  Sender's target ID (0 = K_ANY).
 * @param recv_id  Receiver's source ID (0 = K_ANY).
 *
 * @return 1 if IDs match, 0 if they do not match.
 */
int32_t gale_mbox_match_check(uint32_t send_id, uint32_t recv_id);

/**
 * Compute the actual data exchange size.
 *
 * @param tx_size      Transmit message data size.
 * @param rx_buf_size  Receive buffer size.
 *
 * @return min(tx_size, rx_buf_size).
 */
uint32_t gale_mbox_data_exchange(uint32_t tx_size, uint32_t rx_buf_size);

#ifdef __cplusplus
}
#endif

#endif /* GALE_MBOX_H */
