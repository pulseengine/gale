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
//! Zephyr MMU flag constants (K_MEM_PERM_*, K_MEM_CACHE_*, K_MEM_MAP_*):
//!   include/zephyr/sys/mem_manage.h
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
/// Default page size (4 KiB).  Matches CONFIG_MMU_PAGE_SIZE typical value.
/// The C side passes the actual runtime page size; here we model the default.
pub const PAGE_SIZE: u32 = 4096;
/// Permission: read access.
pub const K_MEM_PERM_RW: u32 = 0x0002;
/// Permission: execute access.
pub const K_MEM_PERM_EXEC: u32 = 0x0004;
/// Permission: user-mode access.
pub const K_MEM_PERM_USER: u32 = 0x0008;
/// Cache policy: write-back.
pub const K_MEM_CACHE_WB: u32 = 0x0100;
/// Cache policy: write-through.
pub const K_MEM_CACHE_WT: u32 = 0x0200;
/// Cache policy: no cache.
pub const K_MEM_CACHE_NONE: u32 = 0x0400;
/// Mask covering all cache policy bits.
pub const K_MEM_CACHE_MASK: u32 = 0x0700;
/// Map flag: pin physical pages (prevent eviction).
pub const K_MEM_MAP_LOCK: u32 = 0x1000;
/// Map flag: do not zero-initialise mapped memory.
pub const K_MEM_MAP_UNINIT: u32 = 0x2000;
/// Map flag: direct physical-to-virtual mapping.
pub const K_MEM_DIRECT_MAP: u32 = 0x4000;
/// Map flag: demand-paged (not immediately backed).
pub const K_MEM_MAP_UNPAGED: u32 = 0x8000;
/// A validated request to map a physical region into virtual address space.
///
/// Corresponds to the validated parameters of k_mem_map_phys_guard
/// (mmu.c:570-677) after all pre-condition checks pass.
#[derive(Clone, Copy)]
pub struct MapRequest {
    /// Physical address to map (may be 0 for anonymous mappings).
    pub phys: u32,
    /// Size of the mapping in bytes.  Must be a non-zero multiple of PAGE_SIZE.
    pub size: u32,
    /// Combined K_MEM_PERM_* | K_MEM_CACHE_* | K_MEM_MAP_* flags.
    pub flags: u32,
    /// True if this is an anonymous (no fixed physical) mapping.
    pub is_anon: bool,
}
/// Validate size alignment: size > 0 and size is a multiple of page_size.
///
/// Mirrors mmu.c:589-596:
///   if ((size % CONFIG_MMU_PAGE_SIZE) != 0U) { return NULL; }
///   if (size == 0) { return NULL; }
pub fn validate_size(size: u32, page_size: u32) -> bool {
    size > 0 && page_size > 0 && (size % page_size) == 0
}
/// Validate that user+uninit flags are not both set.
///
/// mmu.c:584-587.
pub fn validate_user_uninit(flags: u32) -> bool {
    !((flags & K_MEM_PERM_USER) != 0 && (flags & K_MEM_MAP_UNINIT) != 0)
}
/// Validate cache flags: at most one cache policy bit is set.
pub fn validate_cache_flags(flags: u32) -> bool {
    let cache_bits = flags & K_MEM_CACHE_MASK;
    cache_bits == 0 || cache_bits == K_MEM_CACHE_WB || cache_bits == K_MEM_CACHE_WT
        || cache_bits == K_MEM_CACHE_NONE
}
/// Check that size + 2 guard pages does not overflow u32.
///
/// Mirrors mmu.c:601-604:
///   if (size_add_overflow(size, CONFIG_MMU_PAGE_SIZE * 2, &total_size)) { return NULL; }
///
/// We use u64 arithmetic to detect the overflow without triggering it.
pub fn validate_guard_total(size: u32, page_size: u32) -> bool {
    let total: u64 = size as u64 + 2u64 * (page_size as u64);
    total <= u32::MAX as u64
}
/// Full map-request validation.
///
/// Returns true when all pre-conditions from k_mem_map_phys_guard are met:
///   MM1: size > 0 and page-aligned
///   MM2: user+uninit not combined
///   MM5: guard total fits
///
/// Cache-flag validation (MM3) is a separate call because k_mem_map_phys_guard
/// explicitly logs the error differently and it applies only to guard mappings.
pub fn validate_map_request(size: u32, flags: u32, page_size: u32) -> bool {
    validate_size(size, page_size) && validate_user_uninit(flags)
        && validate_guard_total(size, page_size)
}
/// Result of aligning an address/size pair to a page boundary.
///
/// Corresponds to k_mem_region_align (mmu.c:1008-1021):
///   aligned_addr = ROUND_DOWN(addr, align)
///   addr_offset  = addr - aligned_addr
///   aligned_size = ROUND_UP(size + addr_offset, align)
#[derive(Clone, Copy)]
pub struct AlignResult {
    /// Aligned (rounded-down) address.
    pub aligned_addr: u32,
    /// Offset from aligned_addr to the original addr.
    pub addr_offset: u32,
    /// Aligned (rounded-up) total size covering the original range.
    pub aligned_size: u32,
}
/// Compute the aligned address, offset, and size for a physical region.
///
/// Safe: all arithmetic is done in u64 to catch overflow before truncating.
///
/// Precondition: addr + size does not exceed u32::MAX (caller ensures this
/// for physical addresses sourced from valid Zephyr page frames).
pub fn region_align_decide(addr: u32, size: u32, align: u32) -> AlignResult {
    let aligned_addr = (addr / align) * align;
    let addr_offset = addr - aligned_addr;
    let raw: u64 = (size as u64 + addr_offset as u64 + align as u64 - 1u64)
        / align as u64 * align as u64;
    let aligned_size = if raw > u32::MAX as u64 { u32::MAX } else { raw as u32 };
    AlignResult {
        aligned_addr,
        addr_offset,
        aligned_size,
    }
}
/// A virtual address region [base, base+size).
///
/// Reuses the same overlap concept as mpu.rs (MpuRegion) but specialized
/// for the MMU where addresses are u32 virtual addresses.
#[derive(Clone, Copy)]
pub struct VirtRegion {
    /// Base virtual address.
    pub base: u32,
    /// Size in bytes (must be > 0 for a live region).
    pub size: u32,
}
impl VirtRegion {
    /// Runtime overlap check (MM7).
    ///
    /// Uses u64 arithmetic to avoid any overflow on 32-bit address values.
    pub fn overlaps(&self, other: &VirtRegion) -> bool {
        let self_end: u64 = self.base as u64 + self.size as u64;
        let other_end: u64 = other.base as u64 + other.size as u64;
        self_end > other.base as u64 && other_end > self.base as u64
    }
}
/// The set of all known permission/cache/map flag bits.
pub const ALL_KNOWN_FLAGS: u32 = K_MEM_PERM_RW | K_MEM_PERM_EXEC | K_MEM_PERM_USER
    | K_MEM_CACHE_MASK | K_MEM_MAP_LOCK | K_MEM_MAP_UNINIT | K_MEM_DIRECT_MAP
    | K_MEM_MAP_UNPAGED;
/// Check that no unknown flag bits are set.
pub fn validate_flags_known(flags: u32) -> bool {
    (flags & !ALL_KNOWN_FLAGS) == 0
}
pub fn validate_wxor(flags: u32) -> bool {
    !((flags & K_MEM_PERM_RW) != 0 && (flags & K_MEM_PERM_EXEC) != 0)
}
/// Validate an unmap request.
///
/// Mirrors k_mem_unmap_phys_guard pre-conditions (mmu.c:679-695):
///   - addr >= page_size (space for the "before" guard page)
///   - size > 0 and page-aligned
///   - size + 2*page_size does not overflow
pub fn validate_unmap_request(addr: u32, size: u32, page_size: u32) -> bool {
    (addr as u64 >= page_size as u64) && validate_size(size, page_size)
        && validate_guard_total(size, page_size)
}
/// Validate a flags-update request.
///
/// k_mem_update_flags (mmu.c:819-847) requires:
///   - size > 0 and page-aligned
///   - no unknown flag bits
pub fn validate_update_flags(size: u32, flags: u32, page_size: u32) -> bool {
    validate_size(size, page_size) && validate_flags_known(flags)
}
/// MM1: zero size fails validation regardless of alignment.
/// MM2: user+uninit combination is always invalid.
/// MM7: overlap is symmetric.
/// MM7: adjacent regions do not overlap.
/// MM3: no cache policy bits set is a valid (uncached system-default) state.
/// MM5: guard page total overflow check is conservative.
/// Decide whether a map request is valid.
///
/// MM1, MM2, MM5.  Returns 0 on success, negative errno on failure.
/// The C shim calls this before allocating virtual address space.
pub fn map_request_decide(size: u32, flags: u32, page_size: u32) -> i32 {
    use crate::error::{EINVAL, OK};
    if validate_map_request(size, flags, page_size) { OK } else { EINVAL }
}
/// Decide whether an unmap request is valid.
///
/// Returns 0 on success, negative errno on failure.
pub fn unmap_request_decide(addr: u32, size: u32, page_size: u32) -> i32 {
    use crate::error::{EINVAL, OK};
    if validate_unmap_request(addr, size, page_size) { OK } else { EINVAL }
}
/// Decide overlap between two virtual regions.
///
/// Returns true if [base1, base1+size1) and [base2, base2+size2) overlap.
/// Used by the C shim to detect double-mapping of virtual address ranges.
pub fn virt_regions_overlap_decide(
    base1: u32,
    size1: u32,
    base2: u32,
    size2: u32,
) -> bool {
    let r1 = VirtRegion {
        base: base1,
        size: size1,
    };
    let r2 = VirtRegion {
        base: base2,
        size: size2,
    };
    r1.overlaps(&r2)
}
