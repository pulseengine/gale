/*
 * Copyright (c) 2010-2016 Wind River Systems, Inc.
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale fifo — phase 2: Extract→Decide→Apply pattern.
 *
 * This is the k_fifo portion of kernel/queue.c with put/get rewritten
 * to use Rust decision structs.  C extracts kernel state (spinlock,
 * wait queue side effects), Rust decides the action, C applies it.
 *
 * k_fifo is a FIFO ordering wrapper around k_queue — the queue_insert
 * and k_queue_get functions in gale_queue.c call the fifo decision
 * functions when operating in FIFO mode.
 *
 * Verified operations (Verus proofs):
 *   gale_k_fifo_put_decide — FI1 (no overflow), FI2 (increment)
 *   gale_k_fifo_get_decide — FI3 (no underflow), FI4 (decrement)
 *
 * Extract→Decide→Apply flow for fifo put (queue_insert, FIFO mode):
 *
 *   // Extract: try to unpend first waiter
 *   struct k_thread *thread = z_unpend_first_thread(&queue->wait_q);
 *
 *   // Decide: Rust determines action
 *   struct gale_fifo_put_decision d = gale_k_fifo_put_decide(
 *       count, thread != NULL ? 1U : 0U);
 *
 *   // Apply: execute Rust's decision
 *   if (d.action == GALE_FIFO_PUT_WAKE) {
 *       prepare_thread_to_run(thread, data);
 *   } else {
 *       sys_sflist_insert(&queue->data_q, prev, data);
 *   }
 *
 * Extract→Decide→Apply flow for fifo get (k_queue_get, FIFO mode):
 *
 *   // Extract: check if data available
 *   bool has_data = !sys_sflist_is_empty(&queue->data_q);
 *
 *   // Decide: Rust determines action
 *   struct gale_fifo_get_decision d = gale_k_fifo_get_decide(
 *       has_data ? 1U : 0U,
 *       K_TIMEOUT_EQ(timeout, K_NO_WAIT) ? 1U : 0U);
 *
 *   // Apply: execute Rust's decision
 *   if (d.action == GALE_FIFO_GET_OK) {
 *       node = sys_sflist_get_not_empty(&queue->data_q);
 *       data = z_queue_node_peek(node, true);
 *   } else if (d.action == GALE_FIFO_GET_PEND) {
 *       ret = z_pend_curr(&queue->lock, key, &queue->wait_q, timeout);
 *   } else {
 *       data = NULL;  // RETURN_NODATA
 *   }
 */

#include <zephyr/kernel.h>
#include <zephyr/kernel_structs.h>

#include <zephyr/toolchain.h>
#include <wait_q.h>
#include <ksched.h>
#include <zephyr/init.h>
#include <zephyr/internal/syscall_handler.h>
#include <kernel_internal.h>
#include <zephyr/sys/check.h>

#include "gale_fifo.h"

/*
 * NOTE: k_fifo is a macro wrapper around k_queue.  The fifo-specific
 * Gale decision functions (gale_k_fifo_put_decide / gale_k_fifo_get_decide)
 * are called from the queue shim (gale_queue.c) when the queue is used
 * in FIFO mode.  This file exists to provide the fifo-specific FFI
 * entry points and ensure the gale_fifo FFI symbols are linked.
 *
 * The actual queue.c replacement that calls these functions is in
 * gale_queue.c.  This file is compiled separately to ensure the
 * gale_fifo FFI symbols are linked.
 */
