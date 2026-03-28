/*
 * Gale Ring Buffer FFI — verified index arithmetic for ring_buffer.c.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#ifndef GALE_RING_BUF_H
#define GALE_RING_BUF_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ---- Claim decision ---- */

struct gale_ring_buf_claim_decision {
    uint32_t claim_size;     /* safe number of contiguous bytes */
    uint32_t buffer_offset;  /* physical offset into buffer array */
};

struct gale_ring_buf_claim_decision gale_ring_buf_claim_decide(
    uint32_t head, uint32_t base, uint32_t buf_size, uint32_t requested);

/* ---- Finish validation ---- */

int32_t gale_ring_buf_finish_validate(
    uint32_t head, uint32_t tail, uint32_t size, uint32_t buf_size);

/* ---- Space/size queries ---- */

uint32_t gale_ring_buf_space_get(
    uint32_t put_head, uint32_t get_tail, uint32_t buf_size);

uint32_t gale_ring_buf_size_get(
    uint32_t put_tail, uint32_t get_head);

#ifdef __cplusplus
}
#endif

#endif /* GALE_RING_BUF_H */
