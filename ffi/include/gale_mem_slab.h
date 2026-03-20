/*
 * Gale Memory Slab FFI — verified block count arithmetic.
 *
 * These functions replace the block count tracking
 * in kernel/mem_slab.c.  The C shim reads num_used and
 * num_blocks from the slab's info struct:
 *   num_used  = slab->info.num_used
 *   num_blocks = slab->info.num_blocks
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#ifndef GALE_MEM_SLAB_H
#define GALE_MEM_SLAB_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Validate memory slab init parameters.
 *
 * @param block_size  Size of each block in bytes.
 * @param num_blocks  Number of blocks in the slab.
 *
 * @return 0 on success, -EINVAL if block_size == 0 or num_blocks == 0.
 */
int32_t gale_mem_slab_init_validate(uint32_t block_size, uint32_t num_blocks);

/**
 * Validate an alloc operation and compute new num_used.
 *
 * Caller takes the block from the free list before advancing:
 *   *mem = slab->free_list;
 *   slab->free_list = *(char **)(slab->free_list);
 *
 * @param num_used     Current allocated block count.
 * @param num_blocks   Total blocks in the slab.
 * @param new_num_used Output: num_used + 1.
 *
 * @return 0 on success, -ENOMEM if slab full.
 */
int32_t gale_mem_slab_alloc_validate(uint32_t num_used,
                                      uint32_t num_blocks,
                                      uint32_t *new_num_used);

/**
 * Validate a free operation and compute new num_used.
 *
 * Caller returns the block to the free list after decrementing:
 *   *(char **) mem = slab->free_list;
 *   slab->free_list = (char *) mem;
 *
 * @param num_used     Current allocated block count.
 * @param new_num_used Output: num_used - 1.
 *
 * @return 0 on success, -EINVAL if all blocks already free.
 */
int32_t gale_mem_slab_free_validate(uint32_t num_used,
                                     uint32_t *new_num_used);

/* ---- Phase 2: Full Decision API ---- */

struct gale_mem_slab_alloc_decision {
    int32_t  ret;          /* 0=OK, -ENOMEM=slab full */
    uint32_t new_num_used;
    uint8_t  action;       /* 0=ALLOC_OK, 1=PEND_CURRENT, 2=RETURN_NOMEM */
};

#define GALE_MEM_SLAB_ACTION_ALLOC_OK     0
#define GALE_MEM_SLAB_ACTION_PEND_CURRENT 1
#define GALE_MEM_SLAB_ACTION_RETURN_NOMEM 2

struct gale_mem_slab_alloc_decision gale_k_mem_slab_alloc_decide(
    uint32_t num_used, uint32_t num_blocks, uint32_t is_no_wait);

struct gale_mem_slab_free_decision {
    uint32_t new_num_used;
    uint8_t  action;       /* 0=FREE_OK, 1=WAKE_THREAD */
};

#define GALE_MEM_SLAB_ACTION_FREE_OK      0
#define GALE_MEM_SLAB_ACTION_WAKE_THREAD  1

struct gale_mem_slab_free_decision gale_k_mem_slab_free_decide(
    uint32_t num_used, uint32_t has_waiter);

#ifdef __cplusplus
}
#endif

#endif /* GALE_MEM_SLAB_H */
