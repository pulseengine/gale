/*
 * Copyright (c) 2015-2019 Intel Corporation
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale net_buf — Extract→Decide→Apply shim for lib/net_buf/buf.c
 * and lib/net_buf/buf_simple.c.
 *
 * This shim provides validated wrapper helpers that call Rust-verified
 * decision functions before performing net_buf operations. The actual
 * k_lifo free list, k_spinlock, fragment chain traversal, DMA data
 * callbacks, and memory management remain in the upstream buf.c.
 *
 * Pattern:
 *   Extract: pool/buffer state (allocated, capacity, ref_count,
 *            head_offset, len, size)
 *   Decide:  call gale_net_buf_*() — returns validated new state
 *   Apply:   use the validated result in the C net_buf implementation
 *
 * Verified operations (Verus + Kani proofs):
 *   NB1: alloc never exceeds pool capacity
 *   NB2: free returns buffer to pool
 *   NB3: ref count tracks owners
 *   NB4: data bounds: head_offset + len <= size
 *   NB5: push/pull preserve bounds (headroom and tailroom checks)
 *   NB6: no double-free (ref_count must be >= 1 to unref)
 */

#include <zephyr/kernel.h>
#include <zephyr/net_buf.h>

#include "gale_net_buf.h"

/*
 * gale_net_buf_pool_alloc_checked — validated pool buffer acquisition.
 *
 * Wraps the uninit_count / k_lifo_get path with a Rust-verified bounds
 * check. Returns 0 if the pool has capacity, -ENOMEM otherwise.
 *
 * Called by net_buf_alloc_len() before acquiring a buffer from the pool.
 * Verified: NB1 (allocated <= capacity).
 */
int32_t gale_net_buf_pool_alloc_checked(uint16_t allocated,
                                         uint16_t capacity)
{
	/* Decide */
	struct gale_net_buf_alloc_decision d =
		gale_net_buf_alloc_decide(allocated, capacity);

	/* Return rc — caller uses new_allocated to update pool state */
	return d.rc;
}

/*
 * gale_net_buf_pool_free_checked — validated pool buffer return.
 *
 * Wraps net_buf_unref with a Rust-verified free guard.
 * Returns 0 on valid free, -EINVAL on double-free attempt.
 *
 * Verified: NB2 (free decrements), NB6 (no double-free).
 */
int32_t gale_net_buf_pool_free_checked(uint16_t allocated)
{
	return gale_net_buf_free_decide(allocated);
}

/*
 * gale_net_buf_ref_checked — validated reference count increment.
 *
 * Wraps net_buf_ref(). Returns new ref_count on success.
 * Verified: NB3 (ref count tracks owners).
 */
uint8_t gale_net_buf_ref_checked(uint8_t ref_count, int32_t *rc_out)
{
	struct gale_net_buf_ref_decision d =
		gale_net_buf_ref_decide(ref_count);

	if (rc_out != NULL) {
		*rc_out = d.rc;
	}

	return d.new_ref_count;
}

/*
 * gale_net_buf_unref_checked — validated reference count decrement.
 *
 * Wraps net_buf_unref(). Returns 1 if the buffer should be freed
 * (ref_count reached 0), 0 otherwise.
 * Verified: NB3 (ref count tracks owners), NB6 (no double-free).
 */
uint8_t gale_net_buf_unref_checked(uint8_t ref_count, int32_t *rc_out)
{
	struct gale_net_buf_ref_decision d =
		gale_net_buf_unref_decide(ref_count);

	if (rc_out != NULL) {
		*rc_out = d.rc;
	}

	return d.should_free;
}

/*
 * gale_net_buf_add_checked — validated net_buf_simple_add.
 *
 * Verifies tailroom >= bytes before extending len.
 * Returns new len on success, 0 on failure (sets rc_out to -ENOMEM).
 *
 * Verified: NB4 (head_offset + new_len <= size), NB5 (tailroom check).
 */
uint16_t gale_net_buf_add_checked(uint16_t head_offset, uint16_t len,
				   uint16_t size, uint16_t bytes,
				   int32_t *rc_out)
{
	struct gale_net_buf_data_decision d =
		gale_net_buf_add_decide(head_offset, len, size, bytes);

	if (rc_out != NULL) {
		*rc_out = d.rc;
	}

	return d.new_len;
}

/*
 * gale_net_buf_remove_checked — validated net_buf_simple_remove_mem.
 *
 * Verifies len >= bytes before shrinking len.
 * Returns new len on success.
 *
 * Verified: NB4/NB5 (len >= bytes check, no underflow).
 */
uint16_t gale_net_buf_remove_checked(uint16_t head_offset, uint16_t len,
				      uint16_t bytes, int32_t *rc_out)
{
	struct gale_net_buf_data_decision d =
		gale_net_buf_remove_decide(head_offset, len, bytes);

	if (rc_out != NULL) {
		*rc_out = d.rc;
	}

	return d.new_len;
}

/*
 * gale_net_buf_push_checked — validated net_buf_simple_push.
 *
 * Verifies headroom (head_offset) >= bytes before moving data pointer.
 * Updates *head_offset_out and *len_out on success.
 *
 * Verified: NB4 (bounds preserved after push), NB5 (headroom >= bytes).
 */
int32_t gale_net_buf_push_checked(uint16_t head_offset, uint16_t len,
				   uint16_t bytes,
				   uint16_t *head_offset_out,
				   uint16_t *len_out)
{
	struct gale_net_buf_data_decision d =
		gale_net_buf_push_decide(head_offset, len, bytes);

	if (d.rc == 0 && head_offset_out != NULL && len_out != NULL) {
		*head_offset_out = d.new_head_offset;
		*len_out         = d.new_len;
	}

	return d.rc;
}

/*
 * gale_net_buf_pull_checked — validated net_buf_simple_pull.
 *
 * Verifies len >= bytes before advancing data pointer.
 * Updates *head_offset_out and *len_out on success.
 *
 * Verified: NB4 (bounds preserved after pull), NB5 (len >= bytes).
 */
int32_t gale_net_buf_pull_checked(uint16_t head_offset, uint16_t len,
				   uint16_t size, uint16_t bytes,
				   uint16_t *head_offset_out,
				   uint16_t *len_out)
{
	struct gale_net_buf_data_decision d =
		gale_net_buf_pull_decide(head_offset, len, size, bytes);

	if (d.rc == 0 && head_offset_out != NULL && len_out != NULL) {
		*head_offset_out = d.new_head_offset;
		*len_out         = d.new_len;
	}

	return d.rc;
}
