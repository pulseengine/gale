/*
 * Gale sys_heap FFI — verified chunk-level allocation invariants.
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * These decision functions replace the safety-critical decision points
 * in lib/heap/heap.c. The actual free-list traversal, pointer arithmetic,
 * memory layout, and bucket management remain in C.
 *
 * Verified properties (Verus SMT/Z3):
 *   HP1: allocated_bytes <= capacity (bounds invariant)
 *   HP2: free_chunks + used_chunks == total_chunks (conservation)
 *   HP3: alloc succeeds only when enough free space
 *   HP4: free returns exactly what was allocated
 *   HP5: no double-free (chunk state tracking)
 *   HP6: aligned allocation respects alignment constraints
 *   HP7: no overflow in size calculations
 *   HP8: merge adjacent free chunks maintains invariant
 */

#ifndef GALE_HEAP_H
#define GALE_HEAP_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ---- sys_heap_alloc decision ---- */

struct gale_heap_alloc_decision {
    uint8_t  action;  /* 0=USE_WHOLE, 1=SPLIT_AND_USE, 2=ALLOC_FAILED */
    uint8_t  valid;   /* 1=ok, 0=corruption detected */
};

#define GALE_HEAP_ACTION_USE_WHOLE    0
#define GALE_HEAP_ACTION_SPLIT_AND_USE 1
#define GALE_HEAP_ACTION_ALLOC_FAILED 2

/**
 * Decide alloc action after alloc_chunk() returns.
 *
 * @param found_chunk      1 if alloc_chunk returned non-zero, 0 if failed.
 * @param found_chunk_sz   Size of the found chunk (chunk units).
 * @param needed_chunk_sz  Requested size (chunk units).
 *
 * @return Decision: use whole chunk, split, or fail.
 */
struct gale_heap_alloc_decision gale_sys_heap_alloc_decide(
    uint32_t found_chunk, uint32_t found_chunk_sz, uint32_t needed_chunk_sz);

/* ---- sys_heap_free decision ---- */

struct gale_heap_free_decision {
    uint8_t  action;       /* 0=FREE_AND_COALESCE, 1=FREE_REJECTED */
    uint8_t  merge_right;  /* 1=merge with right neighbor */
    uint8_t  merge_left;   /* 1=merge with left neighbor */
};

#define GALE_HEAP_ACTION_FREE_AND_COALESCE 0
#define GALE_HEAP_ACTION_FREE_REJECTED     1

/**
 * Decide free action: validate chunk state and coalescing strategy.
 *
 * @param chunk_is_used        1 if chunk_used(h, c) is true.
 * @param right_neighbor_free  1 if right neighbor is free.
 * @param left_neighbor_free   1 if left neighbor is free.
 * @param bounds_check_passed  1 if left_chunk(right_chunk(c)) == c.
 *
 * @return Decision: coalesce or reject (double-free/corruption).
 */
struct gale_heap_free_decision gale_sys_heap_free_decide(
    uint32_t chunk_is_used, uint32_t right_neighbor_free,
    uint32_t left_neighbor_free, uint32_t bounds_check_passed);

/* ---- sys_heap_aligned_alloc decision ---- */

struct gale_heap_aligned_alloc_decision {
    uint8_t  action;        /* 0=PLAIN, 1=PADDED, 2=REJECT */
    uint32_t padded_bytes;  /* padded allocation size (valid when action==1) */
};

#define GALE_HEAP_ALIGN_PLAIN  0
#define GALE_HEAP_ALIGN_PADDED 1
#define GALE_HEAP_ALIGN_REJECT 2

/**
 * Decide aligned alloc: validate alignment and compute padded size.
 *
 * @param bytes               Requested allocation size.
 * @param align               Requested alignment (power of 2 or 0).
 * @param chunk_header_bytes  Chunk header size (4 or 8).
 *
 * @return Decision: plain alloc, padded alloc, or reject.
 */
struct gale_heap_aligned_alloc_decision gale_sys_heap_aligned_alloc_decide(
    uint32_t bytes, uint32_t align, uint32_t chunk_header_bytes);

/* ---- sys_heap_realloc decision ---- */

struct gale_heap_realloc_decision {
    uint8_t  action;  /* 0=SHRINK, 1=GROW, 2=COPY, 3=REJECT */
};

#define GALE_HEAP_REALLOC_SHRINK 0
#define GALE_HEAP_REALLOC_GROW   1
#define GALE_HEAP_REALLOC_COPY   2
#define GALE_HEAP_REALLOC_REJECT 3

/**
 * Decide realloc strategy: shrink, grow in-place, or alloc+copy+free.
 *
 * @param current_chunk_sz     Current chunk size (chunk units).
 * @param needed_chunk_sz      New required size (chunk units).
 * @param right_neighbor_free  1 if right neighbor is free.
 * @param right_neighbor_sz    Right neighbor size (chunk units, 0 if N/A).
 *
 * @return Decision: shrink, grow, copy, or reject.
 */
struct gale_heap_realloc_decision gale_sys_heap_realloc_decide(
    uint32_t current_chunk_sz, uint32_t needed_chunk_sz,
    uint32_t right_neighbor_free, uint32_t right_neighbor_sz);

/* ---- Validation helpers ---- */

/**
 * Validate sys_heap_init parameters.
 *
 * @param total_bytes   Raw heap memory size.
 * @param min_overhead  Minimum bytes for z_heap struct + buckets + end marker.
 *
 * @return 0 (OK) if valid, -EINVAL if too small.
 */
int32_t gale_sys_heap_init_validate(uint32_t total_bytes, uint32_t min_overhead);

/**
 * Validate split_chunks preconditions.
 *
 * @param original_sz  Original chunk size (chunk units).
 * @param left_sz      Desired left chunk size (chunk units).
 *
 * @return right_sz on success (> 0), 0 on invalid parameters.
 */
uint32_t gale_sys_heap_split_validate(uint32_t original_sz, uint32_t left_sz);

/**
 * Validate merge_chunks preconditions.
 *
 * @param left_sz     Left chunk size (chunk units).
 * @param right_sz    Right chunk size (chunk units).
 * @param left_free   1 if left is free.
 * @param right_free  1 if right is free.
 * @param merged_sz   Output: merged size.
 *
 * @return 0 (OK) if valid, -EINVAL if not.
 */
int32_t gale_sys_heap_merge_validate(uint32_t left_sz, uint32_t right_sz,
                                      uint32_t left_free, uint32_t right_free,
                                      uint32_t *merged_sz);

#ifdef __cplusplus
}
#endif

#endif /* GALE_HEAP_H */
