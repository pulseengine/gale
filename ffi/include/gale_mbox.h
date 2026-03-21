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

/* ---- Phase 2: Full Decision API ---- */

struct gale_mbox_put_decision {
    uint8_t action;     /* 0=MATCHED, 1=RETURN_ENOMSG, 2=PEND_TX_QUEUE */
};

#define GALE_MBOX_ACTION_MATCHED      0
#define GALE_MBOX_ACTION_RETURN_ENOMSG 1
#define GALE_MBOX_ACTION_PEND_TX      2

struct gale_mbox_put_decision gale_k_mbox_put_decide(
    uint32_t matched, uint32_t is_no_wait);

struct gale_mbox_get_decision {
    uint8_t action;     /* 0=MATCHED, 1=RETURN_ENOMSG, 2=PEND_RX_QUEUE */
};

#define GALE_MBOX_ACTION_CONSUME  0
/* GALE_MBOX_ACTION_RETURN_ENOMSG = 1 (shared with put) */
#define GALE_MBOX_ACTION_PEND_RX  2

struct gale_mbox_get_decision gale_k_mbox_get_decide(
    uint32_t matched, uint32_t is_no_wait);

#ifdef __cplusplus
}
#endif

#endif /* GALE_MBOX_H */
