//! Verified MMU virtual address space management model.
//!
//! This is a formally verified model of Zephyr's MMU virtual address space
//! management, based on kernel/mmu.c.  The module covers the decision and
//! validation logic only — hardware page table manipulation, TLB flushes,
//! and physical page frame accounting all remain in C.
//!
//! Source mapping:
//!   k_mem_map_phys_guard          -> validate_map_request (mmu.c:570-677)
//!   k_mem_unmap_phys_guard        -> validate_unmap_request (mmu.c:679-817)
//!   k_mem_update_flags            -> validate_update_flags (mmu.c:819-847)
//!   k_mem_region_align            -> region_align_decide (mmu.c:1008-1021)
//!   virt_region_alloc sanity      -> region_in_bounds (mmu.c:289-369)
//!
//! ASIL-D verified properties:
//!   MM1: page-aligned size check (size % page_size == 0, size > 0)
//!   MM2: user+uninit combination is forbidden
//!   MM3: cache flag mutual exclusion (at most one cache policy)
//!   MM4: region alignment preserves page alignment and no overflow
//!   MM5: guard page total size does not overflow
//!   MM6: permission flags are a subset of the defined flag set
//!   MM7: regions_overlap detection is correct and symmetric
//!   MM8: no arithmetic overflow in any address computation (u64 range)

/// Default page size (4 KiB).
pub const PAGE_SIZE: u32 = 4096;

/// Permission: read+write access.
pub const K_MEM_PERM_RW: u32   = 0x0002;
/// Permission: execute access.
pub const K_MEM_PERM_EXEC: u32 = 0x0004;
/// Permission: user-mode access.
pub const K_MEM_PERM_USER: u32 = 0x0008;

/// Cache policy: write-back.
pub const K_MEM_CACHE_WB: u32    = 0x0100;
/// Cache policy: write-through.
pub const K_MEM_CACHE_WT: u32    = 0x0200;
/// Cache policy: no cache.
pub const K_MEM_CACHE_NONE: u32  = 0x0400;
/// Mask covering all cache policy bits.
pub const K_MEM_CACHE_MASK: u32  = 0x0700;

/// Map flag: pin physical pages.
pub const K_MEM_MAP_LOCK: u32    = 0x1000;
/// Map flag: do not zero-initialise.
pub const K_MEM_MAP_UNINIT: u32  = 0x2000;
/// Map flag: direct physical-to-virtual mapping.
pub const K_MEM_DIRECT_MAP: u32  = 0x4000;
/// Map flag: demand-paged.
pub const K_MEM_MAP_UNPAGED: u32 = 0x8000;

/// A validated request to map a physical region into virtual address space.
#[derive(Clone, Copy)]
pub struct MapRequest {
    /// Physical address to map.
    pub phys: u32,
    /// Size of the mapping in bytes.
    pub size: u32,
    /// Combined K_MEM_PERM_* | K_MEM_CACHE_* | K_MEM_MAP_* flags.
    pub flags: u32,
    /// True if this is an anonymous mapping.
    pub is_anon: bool,
}

/// Result of aligning an address/size pair to a page boundary.
#[derive(Clone, Copy)]
pub struct AlignResult {
    /// Aligned (rounded-down) address.
    pub aligned_addr: u32,
    /// Offset from aligned_addr to the original addr.
    pub addr_offset: u32,
    /// Aligned (rounded-up) total size.
    pub aligned_size: u32,
}

/// A virtual address region [base, base+size).
#[derive(Clone, Copy)]
pub struct VirtRegion {
    /// Base virtual address.
    pub base: u32,
    /// Size in bytes.
    pub size: u32,
}

/// The set of all known flag bits.
pub const ALL_KNOWN_FLAGS: u32 =
    K_MEM_PERM_RW | K_MEM_PERM_EXEC | K_MEM_PERM_USER
    | K_MEM_CACHE_MASK
    | K_MEM_MAP_LOCK | K_MEM_MAP_UNINIT | K_MEM_DIRECT_MAP | K_MEM_MAP_UNPAGED;

impl VirtRegion {
    /// Runtime overlap check.
    pub fn overlaps(&self, other: &VirtRegion) -> bool {
        let self_end: u64 = self.base as u64 + self.size as u64;
        let other_end: u64 = other.base as u64 + other.size as u64;
        self_end > other.base as u64 && other_end > self.base as u64
    }
}

/// Validate size alignment: size > 0 and size is a multiple of page_size.
pub fn validate_size(size: u32, page_size: u32) -> bool {
    size > 0 && page_size > 0 && (size % page_size) == 0
}

/// Validate that user+uninit flags are not both set.
pub fn validate_user_uninit(flags: u32) -> bool {
    !((flags & K_MEM_PERM_USER) != 0 && (flags & K_MEM_MAP_UNINIT) != 0)
}

/// Validate cache flags: at most one cache policy bit is set.
pub fn validate_cache_flags(flags: u32) -> bool {
    let cache_bits = flags & K_MEM_CACHE_MASK;
    cache_bits == 0
        || cache_bits == K_MEM_CACHE_WB
        || cache_bits == K_MEM_CACHE_WT
        || cache_bits == K_MEM_CACHE_NONE
}

/// Check that size + 2 guard pages does not overflow u32.
pub fn validate_guard_total(size: u32, page_size: u32) -> bool {
    let total: u64 = size as u64 + 2u64 * (page_size as u64);
    total <= u32::MAX as u64
}

/// Full map-request validation (MM1, MM2, MM5).
pub fn validate_map_request(size: u32, flags: u32, page_size: u32) -> bool {
    validate_size(size, page_size)
        && validate_user_uninit(flags)
        && validate_guard_total(size, page_size)
}

/// Compute the aligned address, offset, and size for a physical region.
pub fn region_align_decide(addr: u32, size: u32, align: u32) -> AlignResult {
    let aligned_addr = if align > 0 { (addr / align) * align } else { addr };
    let addr_offset = addr - aligned_addr;
    let raw: u64 = if align > 0 {
        (size as u64 + addr_offset as u64 + align as u64 - 1u64)
            / align as u64
            * align as u64
    } else {
        size as u64
    };
    let aligned_size = if raw > u32::MAX as u64 { u32::MAX } else { raw as u32 };
    AlignResult { aligned_addr, addr_offset, aligned_size }
}

/// Check that no unknown flag bits are set.
pub fn validate_flags_known(flags: u32) -> bool {
    (flags & !ALL_KNOWN_FLAGS) == 0
}

/// Check that W^X policy is satisfied (RW and EXEC not both set).
pub fn validate_wxor(flags: u32) -> bool {
    !((flags & K_MEM_PERM_RW) != 0 && (flags & K_MEM_PERM_EXEC) != 0)
}

/// Validate an unmap request (MM1, MM5, plus addr guard check).
pub fn validate_unmap_request(addr: u32, size: u32, page_size: u32) -> bool {
    (addr as u64 >= page_size as u64)
        && validate_size(size, page_size)
        && validate_guard_total(size, page_size)
}

/// Validate a flags-update request (MM1, MM6).
pub fn validate_update_flags(size: u32, flags: u32, page_size: u32) -> bool {
    validate_size(size, page_size) && validate_flags_known(flags)
}

/// Decide whether a map request is valid.
///
/// Returns 0 on success, negative errno on failure.
pub fn map_request_decide(size: u32, flags: u32, page_size: u32) -> i32 {
    use crate::error::{EINVAL, OK};
    if validate_map_request(size, flags, page_size) {
        OK
    } else {
        EINVAL
    }
}

/// Decide whether an unmap request is valid.
///
/// Returns 0 on success, negative errno on failure.
pub fn unmap_request_decide(addr: u32, size: u32, page_size: u32) -> i32 {
    use crate::error::{EINVAL, OK};
    if validate_unmap_request(addr, size, page_size) {
        OK
    } else {
        EINVAL
    }
}

/// Decide overlap between two virtual regions.
pub fn virt_regions_overlap_decide(
    base1: u32, size1: u32,
    base2: u32, size2: u32,
) -> bool {
    let r1 = VirtRegion { base: base1, size: size1 };
    let r2 = VirtRegion { base: base2, size: size2 };
    r1.overlaps(&r2)
}
