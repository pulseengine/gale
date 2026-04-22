/*
 * Copyright (c) 2020 Intel Corporation
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale MMU — Extract/Decide/Apply pattern.
 *
 * This is kernel/mmu.c with the validation and decision logic delegated
 * to verified Rust code.  C extracts parameters, Rust decides whether the
 * request is valid, C applies the result (VMA allocation, arch page table
 * updates, TLB maintenance).
 *
 * Verified operations (Verus proofs in src/mmu.rs):
 *   gale_mmu_map_request_decide     — MM1, MM2, MM5
 *   gale_mmu_unmap_request_decide   — MM1, MM5, addr guard
 *   gale_mmu_update_flags_decide    — MM1, MM6
 *   gale_mmu_region_align           — MM4, MM8
 *   gale_mmu_regions_overlap        — MM7
 *
 * Hardware page table manipulation, TLB flushes, demand-paging eviction,
 * physical page frame accounting, and spinlock serialization all remain
 * native Zephyr C.
 */

#include <stdint.h>
#include <stdbool.h>
#include <zephyr/kernel.h>
#include <zephyr/sys/check.h>
#include <zephyr/logging/log.h>
LOG_MODULE_DECLARE(os, CONFIG_KERNEL_LOG_LEVEL);

#include <mmu.h>
#include <kernel_arch_interface.h>
#include <zephyr/spinlock.h>

#include "gale_mmu.h"

/*
 * Gale-guarded wrapper for k_mem_map_phys_guard.
 *
 * Pattern: Extract → Decide (Rust) → Apply (C arch layer).
 *
 * The Extract step is trivial here — parameters come in directly.
 * The Decide step calls the Rust-verified validation.
 * The Apply step is delegated to the upstream arch_mem_map / virt_region_alloc.
 */
void *gale_k_mem_map_phys_guard(uintptr_t phys, size_t size,
				uint32_t flags, bool is_anon)
{
	/* Decide: Rust validates size, flags, guard overflow */
	int32_t rc = gale_mmu_map_request_decide(
		(uint32_t)size,
		flags,
		(uint32_t)CONFIG_MMU_PAGE_SIZE);

	if (rc != 0) {
		LOG_ERR("gale_mmu: invalid map request (size=%zu flags=0x%x)",
			size, flags);
		return NULL;
	}

	/*
	 * Apply: delegate to the upstream k_mem_map_phys_guard.
	 * We intentionally call the original C implementation so that
	 * all physical page frame accounting, guard page setup, and
	 * arch_mem_map() remain unchanged.
	 */
	return k_mem_map_phys_guard(phys, size, flags, is_anon);
}

/*
 * Gale-guarded wrapper for k_mem_unmap_phys_guard.
 */
void gale_k_mem_unmap_phys_guard(void *addr, size_t size, bool is_anon)
{
	/* Decide */
	int32_t rc = gale_mmu_unmap_request_decide(
		(uint32_t)(uintptr_t)addr,
		(uint32_t)size,
		(uint32_t)CONFIG_MMU_PAGE_SIZE);

	if (rc != 0) {
		LOG_ERR("gale_mmu: invalid unmap request (addr=%p size=%zu)",
			addr, size);
		return;
	}

	/* Apply */
	k_mem_unmap_phys_guard(addr, size, is_anon);
}

/*
 * Gale-guarded wrapper for k_mem_update_flags.
 */
int gale_k_mem_update_flags(void *addr, size_t size, uint32_t flags)
{
	/* Decide */
	int32_t rc = gale_mmu_update_flags_decide(
		(uint32_t)size,
		flags,
		(uint32_t)CONFIG_MMU_PAGE_SIZE);

	if (rc != 0) {
		LOG_ERR("gale_mmu: invalid update_flags request "
			"(size=%zu flags=0x%x)", size, flags);
		return -EINVAL;
	}

	/* Apply */
	return k_mem_update_flags(addr, size, flags);
}

/*
 * Gale-guarded k_mem_region_align.
 *
 * Calls the Rust-verified alignment arithmetic and writes the result
 * back into the caller's out-parameters, matching the original ABI.
 *
 * UCA U-5: aligned_size == 0 in the Rust result indicates overflow.  We
 * propagate this as *aligned_size = 0 so the caller (which ROUND_UP's
 * a buddy allocation and then indexes into page tables) can reject the
 * request instead of mapping a silently clamped region.
 */
size_t gale_k_mem_region_align(uintptr_t *aligned_addr, size_t *aligned_size,
			       uintptr_t addr, size_t size, size_t align)
{
	struct gale_mmu_align_result r =
		gale_mmu_region_align((uint32_t)addr,
				      (uint32_t)size,
				      (uint32_t)align);

	if (r.aligned_size == 0) {
		/* Overflow or precondition violation — surface the zero so
		 * the caller rejects the mapping.  aligned_addr is left as
		 * the (meaningless) zero from the FFI sentinel struct.
		 */
		LOG_ERR("gale_mmu: region_align overflow "
			"(addr=0x%lx size=%zu align=%zu)",
			(unsigned long)addr, size, align);
	}

	if (aligned_addr != NULL) {
		*aligned_addr = (uintptr_t)r.aligned_addr;
	}
	if (aligned_size != NULL) {
		*aligned_size = (size_t)r.aligned_size;
	}

	return (size_t)r.addr_offset;
}
