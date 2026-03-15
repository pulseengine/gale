/*
 * Copyright (c) 2010-2016 Wind River Systems, Inc.
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale lifo — verified unbounded queue count arithmetic.
 *
 * This is the k_lifo portion of kernel/queue.c with count tracking
 * replaced by calls to the formally verified Rust implementation.
 * k_lifo is a LIFO ordering wrapper around k_queue — this shim
 * validates put/get count transitions using gale_lifo_put_validate
 * and gale_lifo_get_validate.
 *
 * Wait queue, scheduling, linked list management, alloc nodes,
 * polling, and tracing remain native Zephyr.
 *
 * Verified operations (Verus proofs):
 *   gale_lifo_put_validate — LI1 (no overflow), LI2 (increment)
 *   gale_lifo_get_validate — LI3 (no underflow), LI4 (decrement)
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
 * Gale validation functions (gale_lifo_put_validate / gale_lifo_get_validate)
 * are called from the queue shim (gale_queue.c) when the queue is used
 * in LIFO mode.  This file exists to provide the lifo-specific FFI
 * entry points and can be extended with lifo-specific validation logic.
 *
 * The actual queue.c replacement that calls these functions is in
 * gale_queue.c.  This file is compiled separately to ensure the
 * gale_lifo FFI symbols are linked.
 */

/*
 * Lifo-specific helper: validate a put (prepend) and return the new count.
 * This is a thin wrapper that the queue shim calls for LIFO puts.
 */
