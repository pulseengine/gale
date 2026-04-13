//! Integration tests for the MMU virtual address space management model.
//!
//! Exercises the MMU decision and validation logic against the pre-conditions
//! documented in kernel/mmu.c.
//!
//! These tests run under: cargo test, miri, sanitizers.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::wildcard_enum_match_arm,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::shadow_unrelated,
    unused_parens
)]

use gale::mmu::{
    region_align_decide, validate_cache_flags, validate_flags_known,
    validate_guard_total, validate_map_request, validate_size,
    validate_unmap_request, validate_update_flags, validate_user_uninit,
    validate_wxor, virt_regions_overlap_decide, AlignResult, VirtRegion,
    K_MEM_CACHE_MASK, K_MEM_CACHE_NONE, K_MEM_CACHE_WB, K_MEM_CACHE_WT,
    K_MEM_DIRECT_MAP, K_MEM_MAP_LOCK, K_MEM_MAP_UNINIT, K_MEM_MAP_UNPAGED,
    K_MEM_PERM_EXEC, K_MEM_PERM_RW, K_MEM_PERM_USER, PAGE_SIZE,
};

const PS: u32 = PAGE_SIZE; // 4096

// ==========================================================================
// MM1: Size validation
// ==========================================================================

#[test]
fn mm1_page_aligned_size_accepted() {
    assert!(validate_size(PS, PS));
    assert!(validate_size(2 * PS, PS));
    assert!(validate_size(16 * PS, PS));
    assert!(validate_size(1024 * PS, PS));
}

#[test]
fn mm1_zero_size_rejected() {
    assert!(!validate_size(0, PS));
}

#[test]
fn mm1_unaligned_size_rejected() {
    assert!(!validate_size(1, PS));
    assert!(!validate_size(PS - 1, PS));
    assert!(!validate_size(PS + 1, PS));
    assert!(!validate_size(PS + 100, PS));
}

#[test]
fn mm1_one_byte_page_size_accepts_any_positive() {
    assert!(validate_size(1, 1));
    assert!(validate_size(99, 1));
    assert!(validate_size(u32::MAX, 1));
}

// ==========================================================================
// MM2: User + uninit combination
// ==========================================================================

#[test]
fn mm2_user_uninit_rejected() {
    let flags = K_MEM_PERM_USER | K_MEM_MAP_UNINIT;
    assert!(!validate_user_uninit(flags));
}

#[test]
fn mm2_user_alone_accepted() {
    assert!(validate_user_uninit(K_MEM_PERM_USER));
}

#[test]
fn mm2_uninit_alone_accepted() {
    assert!(validate_user_uninit(K_MEM_MAP_UNINIT));
}

#[test]
fn mm2_no_flags_accepted() {
    assert!(validate_user_uninit(0));
}

#[test]
fn mm2_rw_exec_no_user_accepted() {
    assert!(validate_user_uninit(K_MEM_PERM_RW | K_MEM_PERM_EXEC));
}

// ==========================================================================
// MM3: Cache flag validation
// ==========================================================================

#[test]
fn mm3_no_cache_bits_valid() {
    assert!(validate_cache_flags(0));
}

#[test]
fn mm3_wb_alone_valid() {
    assert!(validate_cache_flags(K_MEM_CACHE_WB));
}

#[test]
fn mm3_wt_alone_valid() {
    assert!(validate_cache_flags(K_MEM_CACHE_WT));
}

#[test]
fn mm3_none_alone_valid() {
    assert!(validate_cache_flags(K_MEM_CACHE_NONE));
}

#[test]
fn mm3_two_cache_bits_invalid() {
    assert!(!validate_cache_flags(K_MEM_CACHE_WB | K_MEM_CACHE_WT));
    assert!(!validate_cache_flags(K_MEM_CACHE_WB | K_MEM_CACHE_NONE));
    assert!(!validate_cache_flags(K_MEM_CACHE_WT | K_MEM_CACHE_NONE));
    assert!(!validate_cache_flags(K_MEM_CACHE_MASK));
}

#[test]
fn mm3_perm_flags_do_not_affect_cache_check() {
    // Perm bits outside the cache mask don't pollute the cache check
    assert!(validate_cache_flags(K_MEM_PERM_RW | K_MEM_CACHE_WB));
    assert!(validate_cache_flags(K_MEM_PERM_USER));
}

// ==========================================================================
// MM5: Guard page overflow check
// ==========================================================================

#[test]
fn mm5_small_size_ok() {
    assert!(validate_guard_total(PS, PS));
    assert!(validate_guard_total(16 * PS, PS));
}

#[test]
fn mm5_max_u32_overflows() {
    // u32::MAX + 2*PAGE_SIZE definitely overflows
    assert!(!validate_guard_total(u32::MAX, PS));
}

#[test]
fn mm5_near_limit_accepted() {
    // u32::MAX - 2*PS should still fit
    let size = u32::MAX - 2 * PS;
    assert!(validate_guard_total(size, PS));
}

#[test]
fn mm5_exactly_at_limit() {
    // u32::MAX - 2*PS + 1 would require total = u32::MAX + 1 — rejected
    let size = u32::MAX - 2 * PS + 1;
    assert!(!validate_guard_total(size, PS));
}

// ==========================================================================
// MM6: Known-flag validation
// ==========================================================================

#[test]
fn mm6_known_flags_accepted() {
    assert!(validate_flags_known(0));
    assert!(validate_flags_known(K_MEM_PERM_RW));
    assert!(validate_flags_known(K_MEM_PERM_EXEC));
    assert!(validate_flags_known(K_MEM_PERM_USER));
    assert!(validate_flags_known(K_MEM_CACHE_WB));
    assert!(validate_flags_known(K_MEM_MAP_LOCK));
    assert!(validate_flags_known(K_MEM_MAP_UNINIT));
    assert!(validate_flags_known(K_MEM_DIRECT_MAP));
    assert!(validate_flags_known(K_MEM_MAP_UNPAGED));
    // Combination of all known flags
    let all = K_MEM_PERM_RW | K_MEM_PERM_EXEC | K_MEM_PERM_USER
        | K_MEM_CACHE_WB | K_MEM_MAP_LOCK | K_MEM_MAP_UNINIT
        | K_MEM_DIRECT_MAP | K_MEM_MAP_UNPAGED;
    assert!(validate_flags_known(all));
}

#[test]
fn mm6_unknown_bits_rejected() {
    // Bit 0x0001 is not a defined flag
    assert!(!validate_flags_known(0x0001));
    // High bits not in any group
    assert!(!validate_flags_known(0x0001_0000));
    assert!(!validate_flags_known(0x8000_0000));
}

// ==========================================================================
// W^X: write-xor-execute policy
// ==========================================================================

#[test]
fn wxor_rw_and_exec_rejected() {
    assert!(!validate_wxor(K_MEM_PERM_RW | K_MEM_PERM_EXEC));
}

#[test]
fn wxor_rw_alone_accepted() {
    assert!(validate_wxor(K_MEM_PERM_RW));
}

#[test]
fn wxor_exec_alone_accepted() {
    assert!(validate_wxor(K_MEM_PERM_EXEC));
}

#[test]
fn wxor_neither_accepted() {
    assert!(validate_wxor(0));
    assert!(validate_wxor(K_MEM_PERM_USER));
}

// ==========================================================================
// Full map request validation
// ==========================================================================

#[test]
fn map_request_valid() {
    let flags = K_MEM_PERM_RW | K_MEM_CACHE_WB;
    assert!(validate_map_request(PS, flags, PS));
    assert!(validate_map_request(4 * PS, flags, PS));
}

#[test]
fn map_request_zero_size_rejected() {
    assert!(!validate_map_request(0, K_MEM_PERM_RW, PS));
}

#[test]
fn map_request_unaligned_size_rejected() {
    assert!(!validate_map_request(PS + 1, K_MEM_PERM_RW, PS));
}

#[test]
fn map_request_user_uninit_rejected() {
    let flags = K_MEM_PERM_USER | K_MEM_MAP_UNINIT;
    assert!(!validate_map_request(PS, flags, PS));
}

#[test]
fn map_request_overflow_rejected() {
    assert!(!validate_map_request(u32::MAX, K_MEM_PERM_RW, PS));
}

// ==========================================================================
// Unmap request validation
// ==========================================================================

#[test]
fn unmap_valid_aligned() {
    // addr must be >= page_size (for "before" guard page)
    let addr = 2 * PS;
    assert!(validate_unmap_request(addr, PS, PS));
    assert!(validate_unmap_request(addr, 4 * PS, PS));
}

#[test]
fn unmap_addr_too_small_rejected() {
    // addr < page_size: not enough space for the before-guard
    assert!(!validate_unmap_request(0, PS, PS));
    assert!(!validate_unmap_request(PS - 1, PS, PS));
}

#[test]
fn unmap_zero_size_rejected() {
    assert!(!validate_unmap_request(PS, 0, PS));
}

#[test]
fn unmap_unaligned_size_rejected() {
    assert!(!validate_unmap_request(PS, 1, PS));
    assert!(!validate_unmap_request(PS, PS + 1, PS));
}

#[test]
fn unmap_overflow_rejected() {
    assert!(!validate_unmap_request(PS, u32::MAX, PS));
}

// ==========================================================================
// Update-flags validation
// ==========================================================================

#[test]
fn update_flags_valid() {
    assert!(validate_update_flags(PS, K_MEM_PERM_RW, PS));
    assert!(validate_update_flags(4 * PS, K_MEM_PERM_EXEC, PS));
    assert!(validate_update_flags(PS, 0, PS));
}

#[test]
fn update_flags_zero_size_rejected() {
    assert!(!validate_update_flags(0, K_MEM_PERM_RW, PS));
}

#[test]
fn update_flags_unknown_flags_rejected() {
    assert!(!validate_update_flags(PS, 0x0001_0000, PS));
}

// ==========================================================================
// MM7: Virtual region overlap
// ==========================================================================

#[test]
fn mm7_identical_regions_overlap() {
    assert!(virt_regions_overlap_decide(0x1000, PS, 0x1000, PS));
}

#[test]
fn mm7_adjacent_regions_no_overlap() {
    // [0x1000, 0x2000) and [0x2000, 0x3000) — touching but not overlapping
    assert!(!virt_regions_overlap_decide(0x1000, PS, 0x2000, PS));
}

#[test]
fn mm7_separated_regions_no_overlap() {
    assert!(!virt_regions_overlap_decide(0x1000, PS, 0x3000, PS));
}

#[test]
fn mm7_partial_overlap() {
    // [0x1000, 0x3000) overlaps [0x2000, 0x4000)
    assert!(virt_regions_overlap_decide(0x1000, 2 * PS, 0x2000, 2 * PS));
}

#[test]
fn mm7_contained_overlaps() {
    // Small region fully inside large region
    assert!(virt_regions_overlap_decide(0x0000, 16 * PS, 0x2000, PS));
    assert!(virt_regions_overlap_decide(0x2000, PS, 0x0000, 16 * PS));
}

#[test]
fn mm7_overlap_symmetric() {
    let b1 = 0x1000u32;
    let b2 = 0x1800u32;
    let sz = PS;
    assert_eq!(
        virt_regions_overlap_decide(b1, sz, b2, sz),
        virt_regions_overlap_decide(b2, sz, b1, sz),
    );
}

#[test]
fn mm7_virt_region_method_matches_decide() {
    let r1 = VirtRegion { base: 0x4000, size: PS };
    let r2 = VirtRegion { base: 0x5000, size: PS };
    let r3 = VirtRegion { base: 0x4800, size: PS };
    // adjacent
    assert!(!r1.overlaps(&r2));
    // overlapping
    assert!(r1.overlaps(&r3));
}

// ==========================================================================
// MM4: Region alignment arithmetic
// ==========================================================================

#[test]
fn mm4_already_aligned_unchanged() {
    let r = region_align_decide(0x2000, PS, PS);
    assert_eq!(r.aligned_addr, 0x2000);
    assert_eq!(r.addr_offset, 0);
    assert_eq!(r.aligned_size, PS);
}

#[test]
fn mm4_unaligned_addr_rounds_down() {
    // addr = 0x2100 with page_size = 0x1000: aligned_addr = 0x2000
    let r = region_align_decide(0x2100, PS, PS);
    assert_eq!(r.aligned_addr, 0x2000);
    assert_eq!(r.addr_offset, 0x100);
    // aligned_size covers [0x2000, 0x3100) -> rounds up to 0x2000
    assert!(r.aligned_size >= PS + 0x100);
    assert_eq!(r.aligned_size % PS, 0);
}

#[test]
fn mm4_zero_offset_addr_aligned() {
    let r = region_align_decide(0x0000, 2 * PS, PS);
    assert_eq!(r.aligned_addr, 0x0000);
    assert_eq!(r.addr_offset, 0);
    assert_eq!(r.aligned_size, 2 * PS);
}

#[test]
fn mm4_align_result_covers_original_range() {
    let addr = 0x1234u32;
    let size = 0x1000u32;
    let align = 0x1000u32;
    let r: AlignResult = region_align_decide(addr, size, align);
    // aligned_addr <= addr
    assert!(r.aligned_addr <= addr);
    // aligned_addr + aligned_size >= addr + size
    assert!(r.aligned_addr as u64 + r.aligned_size as u64 >= addr as u64 + size as u64);
}

// ==========================================================================
// Constants
// ==========================================================================

#[test]
fn constants_have_expected_values() {
    assert_eq!(PAGE_SIZE, 4096);
    assert_eq!(K_MEM_PERM_RW, 0x0002);
    assert_eq!(K_MEM_PERM_EXEC, 0x0004);
    assert_eq!(K_MEM_PERM_USER, 0x0008);
    assert_eq!(K_MEM_CACHE_WB, 0x0100);
    assert_eq!(K_MEM_CACHE_WT, 0x0200);
    assert_eq!(K_MEM_CACHE_NONE, 0x0400);
    assert_eq!(K_MEM_CACHE_MASK, 0x0700);
    assert_eq!(K_MEM_MAP_LOCK, 0x1000);
    assert_eq!(K_MEM_MAP_UNINIT, 0x2000);
    assert_eq!(K_MEM_DIRECT_MAP, 0x4000);
    assert_eq!(K_MEM_MAP_UNPAGED, 0x8000);
}

// ==========================================================================
// Edge cases
// ==========================================================================

#[test]
fn edge_map_one_page() {
    assert!(validate_map_request(PS, K_MEM_PERM_RW | K_MEM_CACHE_WB, PS));
}

#[test]
fn edge_large_anonymous_mapping() {
    // 64 MB anonymous mapping with write-back cache
    let size = 64 * 1024 * PS; // 64 MB
    assert!(validate_map_request(size, K_MEM_CACHE_WB, PS));
}

#[test]
fn edge_unmap_large_region() {
    let size = 256 * PS;
    let addr = 2 * PS;
    assert!(validate_unmap_request(addr, size, PS));
}

#[test]
fn edge_zero_size_region_no_overlap_with_anything() {
    // A zero-size region [0x1000, 0x1000) doesn't overlap anything
    assert!(!virt_regions_overlap_decide(0x1000, 0, 0x1000, PS));
    assert!(!virt_regions_overlap_decide(0x1000, PS, 0x1000, 0));
}
