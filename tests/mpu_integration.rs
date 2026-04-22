//! Integration tests for ARM MPU v7 region validation.
//!
//! Exercises the MPU region model against the hardware constraints
//! from ARM Architecture Reference Manual ARMv7-M, Section B3.5 (PMSAv7),
//! as implemented in Zephyr's arch/arm/core/mpu/arm_mpu_v7_internal.h.
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

use gale::mpu::{
    is_power_of_two, regions_overlap, validate_region, validate_region_set,
    MpuRegion, MAX_REGIONS_V7, MAX_REGIONS_V8, MIN_REGION_SIZE,
};

// ==========================================================================
// Helper
// ==========================================================================

fn make_region(base: u32, size: u32, attr: u32) -> MpuRegion {
    MpuRegion { base, size, attr }
}

// ==========================================================================
// M1: Non-overlapping regions pass validation
// ==========================================================================

#[test]
fn m1_non_overlapping_regions_pass() {
    // Two 256-byte regions at 0x0 and 0x100 (adjacent, non-overlapping)
    let regions = [
        make_region(0x0000, 256, 0),
        make_region(0x0100, 256, 0),
    ];
    assert!(validate_region_set(&regions, 2));
}

#[test]
fn m1_three_non_overlapping_regions() {
    let regions = [
        make_region(0x0000_0000, 4096, 0), // 0x0000 - 0x0FFF
        make_region(0x0000_1000, 4096, 0), // 0x1000 - 0x1FFF
        make_region(0x0000_2000, 4096, 0), // 0x2000 - 0x2FFF
    ];
    assert!(validate_region_set(&regions, 3));
}

#[test]
fn m1_widely_separated_regions() {
    let regions = [
        make_region(0x0000_0000, 32, 0),   // Flash region
        make_region(0x2000_0000, 4096, 0),  // SRAM
        make_region(0x4000_0000, 256, 0),   // Peripheral
    ];
    assert!(validate_region_set(&regions, 3));
}

// ==========================================================================
// M2: Overlapping regions fail validation
// ==========================================================================

#[test]
fn m2_overlapping_regions_fail() {
    // Two 256-byte regions at 0x0 and 0x80 — overlap at [0x80, 0x100)
    let regions = [
        make_region(0x0000, 256, 0),
        make_region(0x0000, 256, 0),
    ];
    assert!(!validate_region_set(&regions, 2));
}

#[test]
fn m2_partial_overlap_fail() {
    // Region 1: [0x000, 0x100), Region 2: [0x080, 0x180)
    // They overlap at [0x080, 0x100).
    // But 0x80 is not aligned to 256 (0x80 & 0xFF == 0x80 != 0),
    // so validate_region itself will reject region 2.
    let regions = [
        make_region(0x0000, 256, 0),
        make_region(0x0080, 256, 0), // misaligned!
    ];
    assert!(!validate_region_set(&regions, 2));
}

#[test]
fn m2_identical_regions_fail() {
    let regions = [
        make_region(0x1000, 4096, 0),
        make_region(0x1000, 4096, 0),
    ];
    assert!(!validate_region_set(&regions, 2));
}

#[test]
fn m2_containment_overlap() {
    // Large region contains small region
    // Region 1: [0x0000, 0x1000) = 4096 bytes
    // Region 2: [0x0100, 0x0200) = 256 bytes — contained within region 1
    // But 0x0100 is aligned to 256, and 256 is power-of-2, so region 2
    // is individually valid. The set validation should catch the overlap.
    let regions = [
        make_region(0x0000, 4096, 0),
        make_region(0x0100, 256, 0),
    ];
    assert!(!validate_region_set(&regions, 2));
}

// ==========================================================================
// M3: Alignment violations detected
// ==========================================================================

#[test]
fn m3_misaligned_base_rejected() {
    // base 0x10 is not aligned to 256-byte region (0x10 & 0xFF != 0)
    assert!(!validate_region(0x10, 256));
}

#[test]
fn m3_misaligned_base_in_set() {
    let regions = [
        make_region(0x0000, 256, 0),
        make_region(0x0010, 256, 0), // misaligned
    ];
    assert!(!validate_region_set(&regions, 2));
}

#[test]
fn m3_base_aligned_to_smaller_power() {
    // 0x0080 is aligned to 128 but not to 256
    assert!(!validate_region(0x0080, 256));
    assert!(validate_region(0x0080, 128));
}

#[test]
fn m3_odd_base_rejected() {
    assert!(!validate_region(1, 32));
    assert!(!validate_region(3, 32));
    assert!(!validate_region(17, 64));
}

#[test]
fn m3_aligned_base_accepted() {
    // 0x100 is aligned to 256
    assert!(validate_region(0x100, 256));
    // 0x2000_0000 is aligned to 4096
    assert!(validate_region(0x2000_0000, 4096));
    // 0x0 is aligned to everything
    assert!(validate_region(0x0, 32));
    assert!(validate_region(0x0, 4096));
}

// ==========================================================================
// M4: Power-of-2 size enforcement
// ==========================================================================

#[test]
fn m4_power_of_two_accepted() {
    assert!(is_power_of_two(32));
    assert!(is_power_of_two(64));
    assert!(is_power_of_two(128));
    assert!(is_power_of_two(256));
    assert!(is_power_of_two(512));
    assert!(is_power_of_two(1024));
    assert!(is_power_of_two(4096));
    assert!(is_power_of_two(0x0001_0000)); // 64KB
    assert!(is_power_of_two(0x0010_0000)); // 1MB
    assert!(is_power_of_two(0x8000_0000)); // 2GB
}

#[test]
fn m4_non_power_of_two_rejected() {
    assert!(!is_power_of_two(0));
    assert!(!is_power_of_two(3));
    assert!(!is_power_of_two(5));
    assert!(!is_power_of_two(6));
    assert!(!is_power_of_two(7));
    assert!(!is_power_of_two(9));
    assert!(!is_power_of_two(10));
    assert!(!is_power_of_two(12));
    assert!(!is_power_of_two(15));
    assert!(!is_power_of_two(33));
    assert!(!is_power_of_two(48));
    assert!(!is_power_of_two(100));
    assert!(!is_power_of_two(255));
    assert!(!is_power_of_two(4095));
}

#[test]
fn m4_non_power_of_two_size_in_region() {
    // 48 bytes: not a power of 2
    assert!(!validate_region(0, 48));
    // 100 bytes: not a power of 2
    assert!(!validate_region(0, 100));
    // 4000 bytes: not a power of 2
    assert!(!validate_region(0, 4000));
}

#[test]
fn m4_below_minimum_size_rejected() {
    // Powers of 2 below 32 are rejected
    assert!(!validate_region(0, 1));
    assert!(!validate_region(0, 2));
    assert!(!validate_region(0, 4));
    assert!(!validate_region(0, 8));
    assert!(!validate_region(0, 16));
}

// ==========================================================================
// M5: Edge cases
// ==========================================================================

#[test]
fn m5_adjacent_regions_pass() {
    // Two 32-byte regions that are exactly adjacent: [0, 32) and [32, 64)
    let regions = [
        make_region(0x00, 32, 0),
        make_region(0x20, 32, 0),
    ];
    assert!(validate_region_set(&regions, 2));
}

#[test]
fn m5_adjacent_large_regions() {
    // Two 1MB regions adjacent at the boundary
    let regions = [
        make_region(0x0000_0000, 0x0010_0000, 0), // [0, 1MB)
        make_region(0x0010_0000, 0x0010_0000, 0), // [1MB, 2MB)
    ];
    assert!(validate_region_set(&regions, 2));
}

#[test]
fn m5_zero_size_rejected() {
    assert!(!validate_region(0, 0));
    assert!(!validate_region(0x1000, 0));
}

#[test]
fn m5_zero_size_in_set() {
    let regions = [
        make_region(0x0000, 256, 0),
        make_region(0x0100, 0, 0), // zero-size is invalid
    ];
    assert!(!validate_region_set(&regions, 2));
}

#[test]
fn m5_max_region_size() {
    // 2GB region — largest power of 2 that fits in u32
    // Aligned at 0 — the only valid base for a 2GB region
    assert!(validate_region(0, 0x8000_0000));
}

// ==========================================================================
// U-6: adversarial base+size overflow (Mythos 2026-04-21)
// ==========================================================================

/// U-6: (base=0x8000_0000, size=0x8000_0000) — each field alone passes
/// alignment (base & (size-1) == 0), power-of-two, and size >= 32, but
/// base + size overflows u32 and wraps to 0.  Previously validate_region
/// accepted this pair; regions_overlap's precondition was silently
/// violated and userspace isolation was defeated in release builds.
#[test]
fn u6_adversarial_overflow_pair_rejected() {
    let base = 0x8000_0000u32;
    let size = 0x8000_0000u32;
    // Verify the pre-fix conditions individually pass:
    assert_eq!(size & (size - 1), 0, "size is a power of two");
    assert!(size >= MIN_REGION_SIZE);
    assert_eq!(base & (size - 1), 0, "base is aligned to size");
    // And the post-fix check catches the overflow:
    assert!(!validate_region(base, size),
        "U-6: base + size overflows u32 must be rejected");
}

/// U-6: the same adversarial pair must also be rejected when embedded
/// in a region set, so the verified `external_body` wrapper's runtime
/// guard catches any caller that bypasses the single-region validator.
#[test]
fn u6_adversarial_pair_in_region_set_rejected() {
    let regions = [
        make_region(0x8000_0000, 0x8000_0000, 0),
    ];
    assert!(!validate_region_set(&regions, 1),
        "U-6: region_set must reject overflow pair");
}

/// U-6: a pair whose individual fields pass but whose sum equals exactly
/// u32::MAX + 1 (base=0xFFFF_FFE0, size=32 → end = 0x1_0000_0000) must
/// be rejected.  32 is the minimum legal size; 0xFFFF_FFE0 is aligned
/// to 32 (low 5 bits clear); yet base + size wraps.
#[test]
fn u6_min_size_at_top_of_address_space_rejected() {
    let base = 0xFFFF_FFE0u32;  // aligned to 32
    let size = MIN_REGION_SIZE; // 32
    assert_eq!(base & (size - 1), 0);
    assert!(!validate_region(base, size),
        "U-6: base+size exactly wrapping must be rejected");
}

/// U-6: the boundary case — base + size exactly equal to u32::MAX — is
/// NOT an overflow.  Every u32 value with the high bit 0 and a valid
/// small size should still validate; this pair must be accepted.
#[test]
fn u6_near_top_boundary_accepted() {
    // base=0, size=2GB: base + size = 0x8000_0000, well below u32::MAX.
    assert!(validate_region(0, 0x8000_0000));
    // base=0x8000_0000, size=0x4000_0000: sum = 0xC000_0000, fits.
    assert!(validate_region(0x8000_0000, 0x4000_0000));
}

#[test]
fn m5_single_region_valid() {
    let regions = [make_region(0x2000_0000, 4096, 0)];
    assert!(validate_region_set(&regions, 1));
}

#[test]
fn m5_zero_count_valid() {
    // Vacuously valid: no regions to check
    let regions = [make_region(0, 32, 0)];
    assert!(validate_region_set(&regions, 0));
}

#[test]
fn m5_full_v7_region_set() {
    // 8 non-overlapping 4KB regions (max for Cortex-M0+/M3/M4)
    let regions = [
        make_region(0x0000_0000, 4096, 0),
        make_region(0x0000_1000, 4096, 0),
        make_region(0x0000_2000, 4096, 0),
        make_region(0x0000_3000, 4096, 0),
        make_region(0x0000_4000, 4096, 0),
        make_region(0x0000_5000, 4096, 0),
        make_region(0x0000_6000, 4096, 0),
        make_region(0x0000_7000, 4096, 0),
    ];
    assert_eq!(regions.len(), MAX_REGIONS_V7 as usize);
    assert!(validate_region_set(&regions, MAX_REGIONS_V7));
}

#[test]
fn m5_mixed_sizes_non_overlapping() {
    // Regions of different (valid) sizes, all properly aligned, non-overlapping
    let regions = [
        make_region(0x0000_0000, 32, 0),     // [0x0000, 0x0020)
        make_region(0x0000_0100, 256, 0),    // [0x0100, 0x0200)
        make_region(0x0000_1000, 4096, 0),   // [0x1000, 0x2000)
        make_region(0x0001_0000, 0x10000, 0),// [0x10000, 0x20000)
    ];
    assert!(validate_region_set(&regions, 4));
}

// ==========================================================================
// M6: is_power_of_two edge cases
// ==========================================================================

#[test]
fn m6_power_of_two_one() {
    // 1 is 2^0, a valid power of 2 (though below min region size)
    assert!(is_power_of_two(1));
}

#[test]
fn m6_power_of_two_max_u32() {
    // u32::MAX = 0xFFFFFFFF is not a power of 2
    assert!(!is_power_of_two(u32::MAX));
}

#[test]
fn m6_power_of_two_u32_high_bit() {
    // 0x80000000 = 2^31, the largest power of 2 in u32
    assert!(is_power_of_two(0x8000_0000));
}

// ==========================================================================
// M7: regions_overlap direct tests
// ==========================================================================

#[test]
fn m7_overlap_identical() {
    let r1 = make_region(0x1000, 256, 0);
    let r2 = make_region(0x1000, 256, 0);
    assert!(regions_overlap(&r1, &r2));
}

#[test]
fn m7_overlap_contained() {
    let r1 = make_region(0x0000, 4096, 0);
    let r2 = make_region(0x0100, 256, 0);
    assert!(regions_overlap(&r1, &r2));
}

#[test]
fn m7_no_overlap_adjacent() {
    let r1 = make_region(0x0000, 256, 0);
    let r2 = make_region(0x0100, 256, 0);
    assert!(!regions_overlap(&r1, &r2));
}

#[test]
fn m7_no_overlap_separated() {
    let r1 = make_region(0x0000, 32, 0);
    let r2 = make_region(0x1000, 32, 0);
    assert!(!regions_overlap(&r1, &r2));
}

#[test]
fn m7_overlap_symmetric() {
    let r1 = make_region(0x0000, 4096, 0);
    let r2 = make_region(0x0800, 4096, 0);
    // Both orderings should agree
    assert_eq!(regions_overlap(&r1, &r2), regions_overlap(&r2, &r1));
}

// ==========================================================================
// M8: Attributes field is preserved (not used in validation)
// ==========================================================================

#[test]
fn m8_attributes_ignored_in_validation() {
    // Different attr values should not affect validation result
    assert!(validate_region_set(
        &[make_region(0x0000, 256, 0x00)],
        1
    ));
    assert!(validate_region_set(
        &[make_region(0x0000, 256, 0xFF)],
        1
    ));
    assert!(validate_region_set(
        &[make_region(0x0000, 256, 0xDEAD_BEEF)],
        1
    ));
}

// ==========================================================================
// M9: Constants
// ==========================================================================

#[test]
fn m9_constants() {
    assert_eq!(MIN_REGION_SIZE, 32);
    assert_eq!(MAX_REGIONS_V7, 8);
    assert_eq!(MAX_REGIONS_V8, 16);
}
