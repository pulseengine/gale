/*
 * Copyright (c) 2016 Intel Corporation
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale ring buffer — Extract→Decide→Apply shim for lib/utils/ring_buffer.c.
 *
 * This shim sits alongside the upstream ring_buffer.c (it does NOT replace
 * the whole file).  It intercepts the index arithmetic operations via
 * wrapper functions that delegate to the Gale Rust FFI for bounds validation.
 *
 * Pattern:
 *   Extract: ring buffer index state (head, tail, size, buf_size)
 *   Decide:  call gale_ring_buf_*() — returns validated claim/offset/size
 *   Apply:   use the validated result in the C ring buffer implementation
 *
 * The actual buffer memory, memcpy, dlist, and item-mode wrappers remain
 * in the upstream lib/utils/ring_buffer.c.
 *
 * Verified operations (Verus proofs):
 *   RB1: 0 <= size <= capacity (bounds invariant)
 *   RB2: head < capacity, tail < capacity (index bounds)
 *   RB3: put advances tail = (tail + 1) % capacity
 *   RB4: get advances head = (head + 1) % capacity
 *   RB5: put on full buffer returns error
 *   RB6: get on empty buffer returns error
 *   RB7: size == (tail - head + capacity) % capacity (consistency)
 *   RB8: no overflow in modular arithmetic
 */

#include <zephyr/kernel.h>
#include <zephyr/sys/ring_buffer.h>

#include "gale_ring_buf.h"

/*
 * gale_ring_buf_claim_validated — wrap ring_buf_put_claim / ring_buf_get_claim.
 *
 * Called by the C ring buffer implementation before issuing a claim.
 * Returns the Gale-validated claim size (may be <= requested).
 *
 * This is a helper for testing / instrumentation; in the full integration
 * the claim logic in ring_buffer.c calls gale_ring_buf_claim_decide directly.
 */
uint32_t gale_ring_buf_claim_validated(uint32_t head, uint32_t base,
				       uint32_t buf_size, uint32_t requested)
{
	/* Decide */
	struct gale_ring_buf_claim_decision d =
		gale_ring_buf_claim_decide(head, base, buf_size, requested);

	/* Return validated claim size */
	return d.claim_size;
}

/*
 * gale_ring_buf_offset_validated — return the safe buffer offset.
 *
 * Returns the physical buffer offset for a claim operation.
 * Verified: RB1 (offset < buf_size), RB2 (head wraps correctly).
 */
uint32_t gale_ring_buf_offset_validated(uint32_t head, uint32_t base,
					uint32_t buf_size, uint32_t requested)
{
	struct gale_ring_buf_claim_decision d =
		gale_ring_buf_claim_decide(head, base, buf_size, requested);

	return d.buffer_offset;
}

/*
 * gale_ring_buf_finish_check — validate a finish size before applying.
 *
 * Returns 0 if the finish is valid, -EINVAL if size > claimed.
 * Verified: RB3/RB4 (correct advancement), RB8 (no overflow).
 */
int32_t gale_ring_buf_finish_check(uint32_t head, uint32_t tail,
				   uint32_t size, uint32_t buf_size)
{
	return gale_ring_buf_finish_validate(head, tail, size, buf_size);
}

/*
 * gale_ring_buf_space_validated — compute free space from index state.
 *
 * Thin wrapper so C code can query available space with verified arithmetic.
 * Verified: RB7 (space + size == capacity), RB8 (no overflow).
 */
uint32_t gale_ring_buf_space_validated(uint32_t put_head, uint32_t get_tail,
				       uint32_t buf_size)
{
	return gale_ring_buf_space_get(put_head, get_tail, buf_size);
}

/*
 * gale_ring_buf_size_validated — compute used bytes from index state.
 *
 * Verified: RB7, RB8.
 */
uint32_t gale_ring_buf_size_validated(uint32_t put_tail, uint32_t get_head)
{
	return gale_ring_buf_size_get(put_tail, get_head);
}
