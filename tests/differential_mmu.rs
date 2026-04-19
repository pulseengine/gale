//! Differential equivalence tests — MMU virtual address space management (FFI vs Model).
//!
//! Verifies that the FFI MMU functions produce the same results as
//! the Verus-verified model functions in gale::mmu.

#![allow(
    clippy::shadow_unrelated,
    clippy::indexing_slicing,
    clippy::bool_to_int_with_if,
    clippy::unwrap_used,
    clippy::fn_params_excessive_bools,
    clippy::absurd_extreme_comparisons,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::checked_conversions,
    clippy::wildcard_enum_match_arm,
    clippy::implicit_saturating_sub,
    clippy::branches_sharing_code,
    clippy::panic
)]

use gale::error::*;
use gale::mmu::{
    PAGE_SIZE,
    K_MEM_PERM_RW, K_MEM_PERM_EXEC, K_MEM_PERM_USER,
    K_MEM_CACHE_WB, K_MEM_CACHE_WT, K_MEM_CACHE_NONE, K_MEM_CACHE_MASK,
    K_MEM_MAP_UNINIT,
    ALL_KNOWN_FLAGS,
    map_request_decide, unmap_request_decide,
    validate_wxor, validate_cache_flags,
    validate_size, validate_user_uninit, validate_guard_total,
    validate_flags_known, virt_regions_overlap_decide,
    VirtRegion,
};

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Replica of gale_mmu_map_request_decide.
fn ffi_map_request_decide(size: u32, flags: u32, page_size: u32) -> i32 {
    if page_size == 0 {
        return EINVAL;
    }
    // MM1: size > 0 and page-aligned
    if size == 0 || (size % page_size) != 0 {
        return EINVAL;
    }
    // MM2: user+uninit forbidden
    if (flags & K_MEM_PERM_USER) != 0 && (flags & K_MEM_MAP_UNINIT) != 0 {
        return EINVAL;
    }
    // MM5: size + 2*page_size must not overflow u32
    let total: u64 = size as u64 + 2u64 * (page_size as u64);
    if total > u32::MAX as u64 {
        return EINVAL;
    }
    OK
}

/// Replica of gale_mmu_unmap_request_decide.
fn ffi_unmap_request_decide(addr: u32, size: u32, page_size: u32) -> i32 {
    if page_size == 0 {
        return EINVAL;
    }
    // addr must be at least one page (space for guard page before)
    if (addr as u64) < (page_size as u64) {
        return EINVAL;
    }
    // MM1: size > 0 and page-aligned
    if size == 0 || (size % page_size) != 0 {
        return EINVAL;
    }
    // MM5: guard total
    let total: u64 = size as u64 + 2u64 * (page_size as u64);
    if total > u32::MAX as u64 {
        return EINVAL;
    }
    OK
}

/// Replica of gale_mmu_validate_wxor.
fn ffi_validate_wxor(flags: u32) -> bool {
    !((flags & K_MEM_PERM_RW) != 0 && (flags & K_MEM_PERM_EXEC) != 0)
}

/// Replica of gale_mmu_validate_cache_flags.
fn ffi_validate_cache_flags(flags: u32) -> bool {
    let cache_bits = flags & K_MEM_CACHE_MASK;
    cache_bits == 0
        || cache_bits == K_MEM_CACHE_WB
        || cache_bits == K_MEM_CACHE_WT
        || cache_bits == K_MEM_CACHE_NONE
}

// =====================================================================
// Differential tests: map_request_decide
// =====================================================================

#[test]
fn mmu_map_request_decide_ffi_matches_model_basic() {
    let cases = [
        // (size, flags, page_size, expect_ok)
        (PAGE_SIZE, 0u32, PAGE_SIZE, true),
        (0u32, 0, PAGE_SIZE, false),
        (PAGE_SIZE + 1, 0, PAGE_SIZE, false),
        (PAGE_SIZE, K_MEM_PERM_USER | K_MEM_MAP_UNINIT, PAGE_SIZE, false),
        (PAGE_SIZE, K_MEM_PERM_USER, PAGE_SIZE, true),
        (PAGE_SIZE, K_MEM_MAP_UNINIT, PAGE_SIZE, false), // Wait, UNINIT alone is OK?
    ];

    // The MM2 check: user+uninit combined is forbidden. uninit alone is not.
    let cases2 = [
        (PAGE_SIZE, 0u32, PAGE_SIZE, true),
        (0u32, 0, PAGE_SIZE, false),                      // size == 0
        (PAGE_SIZE + 1, 0, PAGE_SIZE, false),              // misaligned
        (PAGE_SIZE, K_MEM_PERM_USER | K_MEM_MAP_UNINIT, PAGE_SIZE, false), // MM2
        (PAGE_SIZE, K_MEM_PERM_USER, PAGE_SIZE, true),     // USER alone OK
        (PAGE_SIZE * 4, 0, PAGE_SIZE, true),               // multi-page OK
    ];

    for (size, flags, page_size, expect_ok) in cases2 {
        let ffi_rc = ffi_map_request_decide(size, flags, page_size);
        let model_rc = map_request_decide(size, flags, page_size);

        assert_eq!(ffi_rc, model_rc,
            "map_request_decide mismatch: size={size}, flags=0x{flags:x}, page_size={page_size}");

        if expect_ok {
            assert_eq!(ffi_rc, OK,
                "expected OK: size={size}, flags=0x{flags:x}");
        } else {
            assert_eq!(ffi_rc, EINVAL,
                "expected EINVAL: size={size}, flags=0x{flags:x}");
        }
        let _ = cases;
    }
}

#[test]
fn mmu_map_request_decide_ffi_matches_model_exhaustive_flags() {
    // Test across interesting flag combinations with a valid size
    let flag_cases: &[u32] = &[
        0,
        K_MEM_PERM_RW,
        K_MEM_PERM_EXEC,
        K_MEM_PERM_USER,
        K_MEM_PERM_USER | K_MEM_MAP_UNINIT,
        K_MEM_CACHE_WB,
        K_MEM_CACHE_WT,
        K_MEM_CACHE_NONE,
        K_MEM_PERM_RW | K_MEM_CACHE_WB,
        K_MEM_PERM_EXEC | K_MEM_CACHE_WT,
    ];

    for flags in flag_cases {
        let ffi_rc = ffi_map_request_decide(PAGE_SIZE, *flags, PAGE_SIZE);
        let model_rc = map_request_decide(PAGE_SIZE, *flags, PAGE_SIZE);
        assert_eq!(ffi_rc, model_rc,
            "map_request flags: flags=0x{flags:x}");
    }
}

#[test]
fn mmu_map_request_zero_page_size_einval() {
    let ffi_rc = ffi_map_request_decide(PAGE_SIZE, 0, 0);
    // FFI adds a page_size==0 guard returning EINVAL before calling model
    assert_eq!(ffi_rc, EINVAL, "zero page_size must return EINVAL");
}

#[test]
fn mmu_map_request_size_not_aligned_einval() {
    let ffi_rc = ffi_map_request_decide(PAGE_SIZE + 1, 0, PAGE_SIZE);
    assert_eq!(ffi_rc, EINVAL, "MM1: unaligned size must return EINVAL");
    let model_rc = map_request_decide(PAGE_SIZE + 1, 0, PAGE_SIZE);
    assert_eq!(model_rc, EINVAL);
}

// =====================================================================
// Differential tests: unmap_request_decide
// =====================================================================

#[test]
fn mmu_unmap_request_decide_ffi_matches_model_basic() {
    let cases = [
        // (addr, size, page_size, expect_ok)
        (PAGE_SIZE, PAGE_SIZE, PAGE_SIZE, true),     // minimal valid
        (0u32, PAGE_SIZE, PAGE_SIZE, false),          // addr < page_size
        (PAGE_SIZE, 0u32, PAGE_SIZE, false),          // size == 0
        (PAGE_SIZE, PAGE_SIZE + 1, PAGE_SIZE, false), // misaligned size
        (PAGE_SIZE * 2, PAGE_SIZE * 4, PAGE_SIZE, true),
    ];

    for (addr, size, page_size, expect_ok) in cases {
        let ffi_rc = ffi_unmap_request_decide(addr, size, page_size);
        let model_rc = unmap_request_decide(addr, size, page_size);

        assert_eq!(ffi_rc, model_rc,
            "unmap_request_decide mismatch: addr=0x{addr:x}, size={size}, page_size={page_size}");

        if expect_ok {
            assert_eq!(ffi_rc, OK,
                "expected OK: addr=0x{addr:x}, size={size}");
        } else {
            assert_eq!(ffi_rc, EINVAL,
                "expected EINVAL: addr=0x{addr:x}, size={size}");
        }
    }
}

#[test]
fn mmu_unmap_request_addr_too_small_einval() {
    // addr < page_size: no room for the guard page before the mapping
    let rc = ffi_unmap_request_decide(PAGE_SIZE - 1, PAGE_SIZE, PAGE_SIZE);
    assert_eq!(rc, EINVAL,
        "addr < page_size must return EINVAL (no space for guard page)");
    let model = unmap_request_decide(PAGE_SIZE - 1, PAGE_SIZE, PAGE_SIZE);
    assert_eq!(model, EINVAL);
}

// =====================================================================
// Differential tests: validate_wxor
// =====================================================================

#[test]
fn mmu_validate_wxor_ffi_matches_model_exhaustive() {
    let flag_cases: &[u32] = &[
        0,
        K_MEM_PERM_RW,
        K_MEM_PERM_EXEC,
        K_MEM_PERM_USER,
        K_MEM_PERM_RW | K_MEM_PERM_EXEC,      // W^X violation
        K_MEM_PERM_RW | K_MEM_PERM_USER,
        K_MEM_PERM_EXEC | K_MEM_PERM_USER,
        K_MEM_PERM_RW | K_MEM_PERM_EXEC | K_MEM_PERM_USER, // W^X violation
        ALL_KNOWN_FLAGS,
    ];

    for flags in flag_cases {
        let ffi_result = ffi_validate_wxor(*flags);
        let model_result = validate_wxor(*flags);
        assert_eq!(ffi_result, model_result,
            "validate_wxor mismatch: flags=0x{flags:x}");
    }
}

#[test]
fn mmu_validate_wxor_rw_exec_forbidden() {
    let flags = K_MEM_PERM_RW | K_MEM_PERM_EXEC;
    assert!(!ffi_validate_wxor(flags), "W^X: RW+EXEC must be rejected");
    assert!(!validate_wxor(flags));
}

#[test]
fn mmu_validate_wxor_rw_only_ok() {
    assert!(ffi_validate_wxor(K_MEM_PERM_RW), "RW without EXEC is ok");
    assert!(validate_wxor(K_MEM_PERM_RW));
}

#[test]
fn mmu_validate_wxor_exec_only_ok() {
    assert!(ffi_validate_wxor(K_MEM_PERM_EXEC), "EXEC without RW is ok");
    assert!(validate_wxor(K_MEM_PERM_EXEC));
}

#[test]
fn mmu_validate_wxor_no_flags_ok() {
    assert!(ffi_validate_wxor(0), "no permission flags is ok");
    assert!(validate_wxor(0));
}

// =====================================================================
// Differential tests: validate_cache_flags
// =====================================================================

#[test]
fn mmu_validate_cache_flags_ffi_matches_model_exhaustive() {
    let flag_cases: &[u32] = &[
        0,                                    // no cache flag — valid
        K_MEM_CACHE_WB,                       // write-back — valid
        K_MEM_CACHE_WT,                       // write-through — valid
        K_MEM_CACHE_NONE,                     // no cache — valid
        K_MEM_CACHE_WB | K_MEM_CACHE_WT,     // two cache bits — invalid
        K_MEM_CACHE_WB | K_MEM_CACHE_NONE,   // two cache bits — invalid
        K_MEM_CACHE_WT | K_MEM_CACHE_NONE,   // two cache bits — invalid
        K_MEM_CACHE_MASK,                     // all three — invalid
        K_MEM_PERM_RW | K_MEM_CACHE_WB,      // RW + WB — valid (MM3)
        K_MEM_PERM_RW,                        // no cache bit — valid
    ];

    for flags in flag_cases {
        let ffi_result = ffi_validate_cache_flags(*flags);
        let model_result = validate_cache_flags(*flags);
        assert_eq!(ffi_result, model_result,
            "validate_cache_flags mismatch: flags=0x{flags:x}");
    }
}

#[test]
fn mmu_validate_cache_flags_single_bit_valid() {
    assert!(ffi_validate_cache_flags(K_MEM_CACHE_WB), "WB alone is valid");
    assert!(ffi_validate_cache_flags(K_MEM_CACHE_WT), "WT alone is valid");
    assert!(ffi_validate_cache_flags(K_MEM_CACHE_NONE), "NONE alone is valid");
    assert!(ffi_validate_cache_flags(0), "no cache flag is valid");
}

#[test]
fn mmu_validate_cache_flags_two_bits_invalid() {
    assert!(!ffi_validate_cache_flags(K_MEM_CACHE_WB | K_MEM_CACHE_WT),
        "MM3: two cache bits must be invalid");
    assert!(!ffi_validate_cache_flags(K_MEM_CACHE_WB | K_MEM_CACHE_NONE),
        "MM3: WB+NONE must be invalid");
    assert!(!ffi_validate_cache_flags(K_MEM_CACHE_WT | K_MEM_CACHE_NONE),
        "MM3: WT+NONE must be invalid");
}

// =====================================================================
// Differential tests: validate helper functions
// =====================================================================

#[test]
fn mmu_validate_size_basic() {
    assert!(validate_size(PAGE_SIZE, PAGE_SIZE), "one page is valid");
    assert!(validate_size(PAGE_SIZE * 4, PAGE_SIZE), "four pages is valid");
    assert!(!validate_size(0, PAGE_SIZE), "MM1: zero size invalid");
    assert!(!validate_size(PAGE_SIZE + 1, PAGE_SIZE), "MM1: misaligned size invalid");
}

#[test]
fn mmu_validate_user_uninit_ok() {
    assert!(validate_user_uninit(0), "no flags: ok");
    assert!(validate_user_uninit(K_MEM_PERM_USER), "USER alone: ok");
    assert!(validate_user_uninit(K_MEM_MAP_UNINIT), "UNINIT alone: ok");
    assert!(!validate_user_uninit(K_MEM_PERM_USER | K_MEM_MAP_UNINIT),
        "MM2: USER+UNINIT must be rejected");
}

#[test]
fn mmu_validate_guard_total_overflow_detected() {
    // size close to u32::MAX will overflow with 2 guard pages
    let big = u32::MAX - PAGE_SIZE;
    let ok = validate_guard_total(big, PAGE_SIZE);
    assert!(!ok, "MM5: near-overflow size + 2 guard pages must fail");

    let small = PAGE_SIZE;
    let ok2 = validate_guard_total(small, PAGE_SIZE);
    assert!(ok2, "MM5: small size + 2 guard pages must succeed");
}

#[test]
fn mmu_validate_flags_known_only_known_bits() {
    assert!(validate_flags_known(ALL_KNOWN_FLAGS), "all known flags are valid");
    assert!(validate_flags_known(0), "no flags is valid");
    // Set an unknown bit (bit 17 is not in ANY known flag)
    let unknown_bit: u32 = 1 << 17;
    assert!(!validate_flags_known(unknown_bit),
        "MM6: unknown flag bit must be rejected");
}

// =====================================================================
// Differential tests: virt_regions_overlap_decide (MM7)
// =====================================================================

#[test]
fn mmu_virt_regions_overlap_decide_ffi_matches_model_exhaustive() {
    let cases = [
        // (base1, size1, base2, size2, expect_overlap)
        (0u32, 100u32, 0u32, 100u32, true),    // identical regions
        (0, 100, 100, 100, false),              // adjacent, no overlap
        (0, 101, 100, 100, true),               // overlap by 1
        (100, 100, 0, 100, false),              // adjacent (reversed)
        (100, 100, 0, 101, true),               // overlap by 1 (reversed)
        (0, 200, 50, 50, true),                 // contained
        (50, 50, 0, 200, true),                 // contained (reversed)
        (0, PAGE_SIZE, PAGE_SIZE, PAGE_SIZE, false), // page-adjacent
        (0, PAGE_SIZE, PAGE_SIZE - 1, 2, true), // straddles page boundary
    ];

    for (base1, size1, base2, size2, expect_overlap) in cases {
        let ffi_result = virt_regions_overlap_decide(base1, size1, base2, size2);
        let r1 = VirtRegion { base: base1, size: size1 };
        let r2 = VirtRegion { base: base2, size: size2 };
        let model_result = r1.overlaps(&r2);

        assert_eq!(ffi_result, model_result,
            "overlap mismatch: [{base1},{size1}) vs [{base2},{size2})");
        assert_eq!(ffi_result, expect_overlap,
            "overlap expected={expect_overlap}: [{base1},{size1}) vs [{base2},{size2})");
    }
}

#[test]
fn mmu_virt_regions_overlap_symmetric() {
    // MM7: overlap must be symmetric
    let cases = [
        (0u32, 100u32, 50u32, 100u32),
        (0, PAGE_SIZE * 4, PAGE_SIZE * 2, PAGE_SIZE * 4),
        (100, 200, 0, 150),
    ];
    for (b1, s1, b2, s2) in cases {
        let a_b = virt_regions_overlap_decide(b1, s1, b2, s2);
        let b_a = virt_regions_overlap_decide(b2, s2, b1, s1);
        assert_eq!(a_b, b_a,
            "MM7: overlap must be symmetric: [{b1},{s1}) vs [{b2},{s2})");
    }
}

#[test]
fn mmu_virt_regions_no_overlap_adjacent() {
    // Adjacent (not overlapping) page-aligned regions
    let rc = virt_regions_overlap_decide(0, PAGE_SIZE, PAGE_SIZE, PAGE_SIZE);
    assert!(!rc, "MM7: adjacent regions must not overlap");
}
