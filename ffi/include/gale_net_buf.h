/*
 * Gale Net Buffer FFI — verified pool allocation tracking and data
 * pointer arithmetic for lib/net_buf/buf.c and buf_simple.c.
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Verified properties:
 *   NB1: alloc never exceeds pool capacity
 *   NB2: free returns buffer to pool
 *   NB3: ref count tracks owners
 *   NB4: data bounds: head_offset + len <= size
 *   NB5: push/pull preserve bounds
 *   NB6: no double-free
 */

#ifndef GALE_NET_BUF_H
#define GALE_NET_BUF_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ------------------------------------------------------------------ */
/* Pool allocation decision                                             */
/* ------------------------------------------------------------------ */

/**
 * struct gale_net_buf_alloc_decision - result of pool alloc validation.
 * @new_allocated: updated allocated count (valid when rc == 0).
 * @rc:            0 on success, -ENOMEM when pool is exhausted.
 */
struct gale_net_buf_alloc_decision {
    uint16_t new_allocated;
    int32_t  rc;
};

/**
 * gale_net_buf_alloc_decide() - decide a net_buf pool allocation.
 *
 * NB1: success when allocated < capacity.
 * NB1: -ENOMEM when pool is exhausted.
 *
 * @allocated: current number of buffers in use.
 * @capacity:  total pool size (buf_count).
 *
 * Return: allocation decision with new_allocated and rc.
 */
struct gale_net_buf_alloc_decision gale_net_buf_alloc_decide(
    uint16_t allocated,
    uint16_t capacity);

/**
 * gale_net_buf_free_decide() - decide a net_buf pool free.
 *
 * NB2: decrements allocated when > 0.
 * NB6: -EINVAL if allocated == 0 (double-free guard).
 *
 * @allocated: current number of buffers in use.
 *
 * Return: 0 on success, -EINVAL on double-free.
 */
int32_t gale_net_buf_free_decide(uint16_t allocated);

/* ------------------------------------------------------------------ */
/* Reference count decision                                            */
/* ------------------------------------------------------------------ */

/**
 * struct gale_net_buf_ref_decision - result of ref/unref operation.
 * @new_ref_count: updated reference count.
 * @should_free:   1 if buffer must be returned to pool (ref reached 0).
 * @rc:            0 = OK, -EINVAL = double-unref, -EOVERFLOW = saturated.
 */
struct gale_net_buf_ref_decision {
    uint8_t  new_ref_count;
    uint8_t  should_free;
    int32_t  rc;
};

/**
 * gale_net_buf_ref_decide() - decide a net_buf_ref (increment ref count).
 *
 * NB3: increments ref_count by 1.
 * Returns -EOVERFLOW if ref_count == UINT8_MAX.
 *
 * @ref_count: current reference count.
 *
 * Return: ref decision.
 */
struct gale_net_buf_ref_decision gale_net_buf_ref_decide(uint8_t ref_count);

/**
 * gale_net_buf_unref_decide() - decide a net_buf_unref (decrement ref count).
 *
 * NB3: decrements ref_count. Sets should_free=1 when count reaches 0.
 * NB6: returns -EINVAL if ref_count is already 0 (double-free guard).
 *
 * @ref_count: current reference count.
 *
 * Return: ref decision with should_free flag.
 */
struct gale_net_buf_ref_decision gale_net_buf_unref_decide(uint8_t ref_count);

/* ------------------------------------------------------------------ */
/* Data operation decisions (add / remove / push / pull)               */
/* ------------------------------------------------------------------ */

/**
 * struct gale_net_buf_data_decision - result of a data pointer operation.
 * @new_head_offset: updated offset of data pointer from __buf.
 * @new_len:         updated data length.
 * @rc:              0 = OK, -ENOMEM = no tailroom, -EINVAL = bounds error.
 */
struct gale_net_buf_data_decision {
    uint16_t new_head_offset;
    uint16_t new_len;
    int32_t  rc;
};

/**
 * gale_net_buf_add_decide() - decide net_buf_simple_add (append at tail).
 *
 * NB4: new head_offset + new_len <= size.
 * NB5: tailroom must be >= bytes.
 *
 * @head_offset: current data pointer offset from __buf.
 * @len:         current data length.
 * @size:        total buffer size.
 * @bytes:       bytes to append.
 *
 * Return: data decision.
 */
struct gale_net_buf_data_decision gale_net_buf_add_decide(
    uint16_t head_offset,
    uint16_t len,
    uint16_t size,
    uint16_t bytes);

/**
 * gale_net_buf_remove_decide() - decide net_buf_simple_remove_mem.
 *
 * NB4/NB5: len must be >= bytes.
 *
 * @head_offset: current data pointer offset (returned unchanged).
 * @len:         current data length.
 * @bytes:       bytes to remove from tail.
 *
 * Return: data decision.
 */
struct gale_net_buf_data_decision gale_net_buf_remove_decide(
    uint16_t head_offset,
    uint16_t len,
    uint16_t bytes);

/**
 * gale_net_buf_push_decide() - decide net_buf_simple_push (prepend at head).
 *
 * NB4: (head_offset - bytes) + (len + bytes) == head_offset + len <= size.
 * NB5: headroom (head_offset) must be >= bytes.
 *
 * @head_offset: current data pointer offset from __buf.
 * @len:         current data length.
 * @bytes:       bytes to prepend.
 *
 * Return: data decision.
 */
struct gale_net_buf_data_decision gale_net_buf_push_decide(
    uint16_t head_offset,
    uint16_t len,
    uint16_t bytes);

/**
 * gale_net_buf_pull_decide() - decide net_buf_simple_pull (consume from head).
 *
 * NB4: (head_offset + bytes) + (len - bytes) == head_offset + len <= size.
 * NB5: len must be >= bytes.
 *
 * @head_offset: current data pointer offset from __buf.
 * @len:         current data length.
 * @size:        total buffer size (for postcondition).
 * @bytes:       bytes to consume.
 *
 * Return: data decision.
 */
struct gale_net_buf_data_decision gale_net_buf_pull_decide(
    uint16_t head_offset,
    uint16_t len,
    uint16_t size,
    uint16_t bytes);

#ifdef __cplusplus
}
#endif

#endif /* GALE_NET_BUF_H */
