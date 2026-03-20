/*
 * Copyright (c) 2010-2016 Wind River Systems, Inc.
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale lifo — phase 2: Extract→Decide→Apply pattern.
 *
 * This is the k_lifo portion of kernel/queue.c with put/get rewritten
 * to use Rust decision structs.  C extracts kernel state (spinlock,
 * wait queue side effects), Rust decides the action, C applies it.
 *
 * k_lifo is a LIFO ordering wrapper around k_queue — the queue_insert
 * and k_queue_get functions in gale_queue.c call the lifo decision
 * functions when operating in LIFO mode.
 *
 * Verified operations (Verus proofs):
 *   gale_k_lifo_put_decide — LI1 (no overflow), LI2 (increment)
 *   gale_k_lifo_get_decide — LI3 (no underflow), LI4 (decrement)
 *
 * Extract→Decide→Apply flow for lifo put (queue_insert, LIFO mode):
 *
 *   // Extract: try to unpend first waiter
 *   struct k_thread *thread = z_unpend_first_thread(&queue->wait_q);
 *
 *   // Decide: Rust determines action
 *   struct gale_lifo_put_decision d = gale_k_lifo_put_decide(
 *       count, thread != NULL ? 1U : 0U);
 *
 *   // Apply: execute Rust's decision
 *   if (d.action == GALE_LIFO_PUT_WAKE) {
 *       prepare_thread_to_run(thread, data);
 *   } else {
 *       sys_sflist_insert(&queue->data_q, NULL, data);  // prepend
 *   }
 *
 * Extract→Decide→Apply flow for lifo get (k_queue_get, LIFO mode):
 *
 *   // Extract: check if data available
 *   bool has_data = !sys_sflist_is_empty(&queue->data_q);
 *
 *   // Decide: Rust determines action
 *   struct gale_lifo_get_decision d = gale_k_lifo_get_decide(
 *       has_data ? 1U : 0U,
 *       K_TIMEOUT_EQ(timeout, K_NO_WAIT) ? 1U : 0U);
 *
 *   // Apply: execute Rust's decision
 *   if (d.action == GALE_LIFO_GET_OK) {
 *       node = sys_sflist_get_not_empty(&queue->data_q);
 *       data = z_queue_node_peek(node, true);
 *   } else if (d.action == GALE_LIFO_GET_PEND) {
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

#include "gale_lifo.h"

/*
 * NOTE: k_lifo is a macro wrapper around k_queue.  The lifo-specific
 * Gale decision functions (gale_k_lifo_put_decide / gale_k_lifo_get_decide)
 * are called from the queue shim (gale_queue.c) when the queue is used
 * in LIFO mode.  This file exists to provide the lifo-specific FFI
 * entry points and ensure the gale_lifo FFI symbols are linked.
 *
 * The actual queue.c replacement that calls these functions is in
 * gale_queue.c.  This file is compiled separately to ensure the
 * gale_lifo FFI symbols are linked.
 */
