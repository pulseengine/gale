/*
 * Gale Memory Domain FFI — verified partition management.
 *
 * These functions replace the partition validation and slot management
 * in kernel/mem_domain.c.  The C shim extracts the partition arrays
 * (start/size) and calls Rust to decide the action.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#ifndef GALE_MEM_DOMAIN_H
#define GALE_MEM_DOMAIN_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Check whether a partition is valid and non-overlapping with existing
 * partitions in the domain.
 *
 * Mirrors check_add_partition (mem_domain.c:24-86).
 *
 * @param part_start      Start address of candidate partition.
 * @param part_size       Size of candidate partition.
 * @param domain_starts   Array of 16 start addresses (existing partitions).
 * @param domain_sizes    Array of 16 sizes (0 = free slot).
 * @param num_partitions  Current active partition count.
 *
 * @return ret=0 (valid), ret=-EINVAL (invalid/overlapping).
 */
struct gale_mem_domain_check_partition_decision {
    int32_t ret;
};

struct gale_mem_domain_check_partition_decision gale_mem_domain_check_partition(
    uint32_t part_start, uint32_t part_size,
    const uint32_t *domain_starts, const uint32_t *domain_sizes,
    uint32_t num_partitions);

/* ---- Phase 2: Full Decision API ---- */

struct gale_mem_domain_add_decision {
    int32_t  ret;                /* 0=OK, -EINVAL, -ENOSPC */
    uint32_t slot;               /* slot index (valid when ret==0) */
    uint32_t new_num_partitions; /* incremented on success */
    uint8_t  action;             /* 0=ADD_OK, 1=RETURN_ERROR */
};

#define GALE_MEM_DOMAIN_ACTION_ADD_OK     0
#define GALE_MEM_DOMAIN_ACTION_ADD_ERROR  1

struct gale_mem_domain_add_decision gale_k_mem_domain_add_partition_decide(
    uint32_t part_start, uint32_t part_size, uint32_t part_attr,
    const uint32_t *domain_starts, const uint32_t *domain_sizes,
    uint32_t num_partitions);

struct gale_mem_domain_remove_decision {
    int32_t  ret;                /* 0=OK, -ENOENT */
    uint32_t slot;               /* slot index (valid when ret==0) */
    uint32_t new_num_partitions; /* decremented on success */
    uint8_t  action;             /* 0=REMOVE_OK, 1=RETURN_ERROR */
};

#define GALE_MEM_DOMAIN_ACTION_REMOVE_OK     0
#define GALE_MEM_DOMAIN_ACTION_REMOVE_ERROR  1

struct gale_mem_domain_remove_decision gale_k_mem_domain_remove_partition_decide(
    uint32_t part_start, uint32_t part_size,
    const uint32_t *domain_starts, const uint32_t *domain_sizes,
    uint32_t num_partitions);

struct gale_mem_domain_init_part_decision {
    int32_t ret;   /* 0=valid, -EINVAL=invalid */
};

struct gale_mem_domain_init_part_decision gale_mem_domain_init_validate_partition(
    uint32_t part_start, uint32_t part_size,
    const uint32_t *domain_starts, const uint32_t *domain_sizes,
    uint32_t num_partitions);

#ifdef __cplusplus
}
#endif

#endif /* GALE_MEM_DOMAIN_H */
