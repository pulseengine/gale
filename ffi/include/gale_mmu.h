/*
 * Gale MMU FFI — verified virtual address space management decisions.
 *
 * These functions replace the validation logic in kernel/mmu.c.
 * The C shim calls Rust to decide whether a map/unmap/update-flags
 * request is valid before committing any virtual address space changes.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#ifndef GALE_MMU_H
#define GALE_MMU_H

#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Validate a map request before allocating virtual address space.
 *
 * Mirrors pre-conditions of k_mem_map_phys_guard (mmu.c:570-677):
 *   MM1: size > 0 and size % page_size == 0
 *   MM2: user+uninit not combined
 *   MM5: size + 2*page_size does not overflow
 *
 * @param size       Mapping size in bytes.
 * @param flags      K_MEM_PERM_* | K_MEM_CACHE_* | K_MEM_MAP_* flags.
 * @param page_size  System page size (CONFIG_MMU_PAGE_SIZE).
 *
 * @return 0 on success, -EINVAL on invalid request.
 */
int32_t gale_mmu_map_request_decide(uint32_t size, uint32_t flags,
                                    uint32_t page_size);

/**
 * Validate an unmap request.
 *
 * Mirrors pre-conditions of k_mem_unmap_phys_guard (mmu.c:679-695):
 *   - addr >= page_size (space for "before" guard page)
 *   - size > 0 and page-aligned
 *   - size + 2*page_size does not overflow
 *
 * @param addr       Virtual address to unmap.
 * @param size       Mapping size in bytes.
 * @param page_size  System page size.
 *
 * @return 0 on success, -EINVAL on invalid request.
 */
int32_t gale_mmu_unmap_request_decide(uint32_t addr, uint32_t size,
                                      uint32_t page_size);

/**
 * Validate a flags-update request.
 *
 * Mirrors k_mem_update_flags pre-conditions (mmu.c:819-847):
 *   - size > 0 and page-aligned
 *   - flags only contain known K_MEM_* bits
 *
 * @param size       Region size in bytes.
 * @param flags      K_MEM_PERM_* | K_MEM_CACHE_* flags to apply.
 * @param page_size  System page size.
 *
 * @return 0 on success, -EINVAL on invalid request.
 */
int32_t gale_mmu_update_flags_decide(uint32_t size, uint32_t flags,
                                     uint32_t page_size);

/**
 * Decision struct for region alignment.
 *
 * Mirrors k_mem_region_align (mmu.c:1008-1021).
 *
 * Overflow signalling (UCA U-5):
 *   aligned_size == 0 indicates the request overflowed u32 (the aligned
 *   region would exceed the 32-bit address space) OR a precondition was
 *   violated (align == 0, addr + size > u32::MAX).  Callers MUST check
 *   aligned_size before using aligned_addr / addr_offset.
 */
struct gale_mmu_align_result {
    uint32_t aligned_addr;  /**< ROUND_DOWN(addr, align); undefined if aligned_size==0 */
    uint32_t addr_offset;   /**< addr - aligned_addr; undefined if aligned_size==0    */
    uint32_t aligned_size;  /**< ROUND_UP(size + addr_offset, align); 0 signals error */
};

/**
 * Compute page-aligned address and size for a physical region.
 *
 * @param addr       Original (possibly unaligned) address.
 * @param size       Original size in bytes.
 * @param align      Alignment boundary (must be > 0).
 *
 * @return AlignResult.  On overflow or invalid input, aligned_size == 0.
 */
struct gale_mmu_align_result gale_mmu_region_align(uint32_t addr, uint32_t size,
                                                   uint32_t align);

/**
 * Check whether two virtual address regions overlap.
 *
 * @param base1  Start of first region.
 * @param size1  Size of first region in bytes.
 * @param base2  Start of second region.
 * @param size2  Size of second region in bytes.
 *
 * @return true if the regions overlap, false otherwise.
 */
bool gale_mmu_regions_overlap(uint32_t base1, uint32_t size1,
                              uint32_t base2, uint32_t size2);

#ifdef __cplusplus
}
#endif

#endif /* GALE_MMU_H */
