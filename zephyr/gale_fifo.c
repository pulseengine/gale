/*
 * Copyright (c) 2010-2016 Wind River Systems, Inc.
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale fifo — verified unbounded queue count arithmetic.
 *
 * This is the k_fifo portion of kernel/queue.c with count tracking
 * replaced by calls to the formally verified Rust implementation.
 * k_fifo is a FIFO ordering wrapper around k_queue — this shim
 * validates put/get count transitions using gale_fifo_put_validate
 * and gale_fifo_get_validate.
 *
 * Wait queue, scheduling, linked list management, alloc nodes,
 * polling, and tracing remain native Zephyr.
 *
 * Verified operations (Verus proofs):
 *   gale_fifo_put_validate — FI1 (no overflow), FI2 (increment)
 *   gale_fifo_get_validate — FI3 (no underflow), FI4 (decrement)
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
 * Gale validation functions (gale_fifo_put_validate / gale_fifo_get_validate)
 * are called from the queue shim (gale_queue.c) when the queue is used
 * in FIFO mode.  This file exists to provide the fifo-specific FFI
 * entry points and can be extended with fifo-specific validation logic.
 *
 * The actual queue.c replacement that calls these functions is in
 * gale_queue.c.  This file is compiled separately to ensure the
 * gale_fifo FFI symbols are linked.
 */

/*
 * Fifo-specific helper: validate a put (append) and return the new count.
 * This is a thin wrapper that the queue shim calls for FIFO puts.
 */
