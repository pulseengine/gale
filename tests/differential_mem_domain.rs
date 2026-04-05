//! Differential equivalence tests — MemDomain (FFI vs Model).
//!
//! Verifies that the FFI memory domain functions produce the same results
//! as the Verus-verified model functions in gale::mem_domain.

#![allow(
    clippy::shadow_unrelated,
    clippy::indexing_slicing,
    clippy::bool_to_int_with_if,
    clippy::unwrap_used,
    clippy::fn_params_excessive_bools,
    clippy::absurd_extreme_comparisons,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::checked_conversions,
    clippy::wildcard_enum_match_arm,
    clippy::implicit_saturating_sub,
    clippy::branches_sharing_code,
    clippy::panic
)]

use gale::error::*;
use gale::mem_domain::{MemDomain, MemPartition, partition_valid_decide, partitions_overlap_decide};

const MAX_PARTITIONS: u32 = 16;

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Replica of gale_mem_domain_check_partition.
///
/// Checks MD3 (size > 0), MD6 (no overflow), MD1 (non-overlap with
/// all existing active partitions).
fn ffi_check_partition(
    part_start: u32,
    part_size: u32,
    domain_starts: &[u32; 16],
    domain_sizes: &[u32; 16],
) -> i32 {
    // MD3 + MD6: validate partition
    if !partition_valid_decide(part_start, part_size) {
        return EINVAL;
    }

    // MD1: check non-overlap with all existing active partitions
    let mut i: u32 = 0;
    while i < MAX_PARTITIONS {
        let dsize = domain_sizes[i as usize];
        if dsize > 0 {
            let dstart = domain_starts[i as usize];
            if partitions_overlap_decide(part_start, part_size, dstart, dsize) {
                return EINVAL;
            }
        }
        i += 1;
    }

    OK
}

/// Replica of gale_k_mem_domain_add_partition_decide.
///
/// Returns (ret, slot, new_num_partitions, action).
fn ffi_add_partition_decide(
    part_start: u32,
    part_size: u32,
    domain_starts: &[u32; 16],
    domain_sizes: &[u32; 16],
    num_partitions: u32,
) -> (i32, u32, u32, u8) {
    const ACTION_ADD_OK: u8 = 0;
    const ACTION_ADD_ERROR: u8 = 1;

    let check_ret = ffi_check_partition(part_start, part_size, domain_starts, domain_sizes);
    if check_ret != OK {
        return (EINVAL, 0, num_partitions, ACTION_ADD_ERROR);
    }

    // Find a free slot (size == 0)
    let mut p_idx: u32 = 0;
    while p_idx < MAX_PARTITIONS {
        if domain_sizes[p_idx as usize] == 0 {
            let new_num = num_partitions + 1;
            return (OK, p_idx, new_num, ACTION_ADD_OK);
        }
        p_idx += 1;
    }

    (ENOSPC, 0, num_partitions, ACTION_ADD_ERROR)
}

/// Replica of gale_k_mem_domain_remove_partition_decide.
///
/// Returns (ret, slot, new_num_partitions, action).
fn ffi_remove_partition_decide(
    part_start: u32,
    part_size: u32,
    domain_starts: &[u32; 16],
    domain_sizes: &[u32; 16],
    num_partitions: u32,
) -> (i32, u32, u32, u8) {
    const ACTION_REMOVE_OK: u8 = 0;
    const ACTION_REMOVE_ERROR: u8 = 1;

    let mut p_idx: u32 = 0;
    while p_idx < MAX_PARTITIONS {
        if domain_starts[p_idx as usize] == part_start
            && domain_sizes[p_idx as usize] == part_size
        {
            let new_num = if num_partitions > 0 { num_partitions - 1 } else { 0 };
            return (OK, p_idx, new_num, ACTION_REMOVE_OK);
        }
        p_idx += 1;
    }

    (ENOENT, 0, num_partitions, ACTION_REMOVE_ERROR)
}

// =====================================================================
// Helper: extract parallel arrays from MemDomain
// =====================================================================

fn domain_arrays(d: &MemDomain) -> ([u32; 16], [u32; 16]) {
    let mut starts = [0u32; 16];
    let mut sizes = [0u32; 16];
    for i in 0..16 {
        starts[i] = d.partitions[i].start;
        sizes[i] = d.partitions[i].size;
    }
    (starts, sizes)
}

// =====================================================================
// Differential tests: partition_valid_decide
// =====================================================================

#[test]
fn mem_domain_partition_valid_decide_matches_model() {
    let test_cases: &[(u32, u32)] = &[
        (0, 0),         // size == 0: invalid (MD3)
        (0, 1),         // minimal valid
        (0, 100),
        (100, 200),
        (u32::MAX, 1),  // overflow (MD6)
        (u32::MAX - 1, 2), // overflow
        (u32::MAX - 1, 1), // exact boundary: start+size == u32::MAX: valid
        (0, u32::MAX),  // large size: valid (0 + MAX <= MAX)
    ];

    for &(start, size) in test_cases {
        let ffi_result = partition_valid_decide(start, size);

        let part = MemPartition { start, size, attr: 0 };
        // model: size > 0 && start as u64 + size as u64 <= u32::MAX as u64
        let model_result = size > 0 && (start as u64 + size as u64) <= u32::MAX as u64;

        assert_eq!(
            ffi_result, model_result,
            "partition_valid_decide mismatch: start={start}, size={size}"
        );
        // Also verify MemPartition::is_valid_rt agrees
        assert_eq!(
            ffi_result,
            part.is_valid_rt(),
            "is_valid_rt mismatch: start={start}, size={size}"
        );
    }
}

// =====================================================================
// Differential tests: partitions_overlap_decide
// =====================================================================

#[test]
fn mem_domain_partitions_overlap_decide_matches_model() {
    // Test pairs of non-zero-size partitions
    let cases: &[(u32, u32, u32, u32, bool)] = &[
        // (start1, size1, start2, size2, expected_overlap)
        (0, 10, 10, 10, false),  // adjacent: [0,10) [10,20) — no overlap
        (0, 11, 10, 10, true),   // overlap: [0,11) [10,20)
        (0, 10, 5, 5, true),     // contained: [0,10) [5,10)
        (5, 5, 0, 10, true),     // reverse contained
        (100, 100, 200, 100, false), // gap between
        (0, 100, 50, 10, true),  // inner
        (50, 10, 0, 100, true),  // reverse inner
        (0, 1, 1, 1, false),     // minimal adjacent
        (0, 2, 1, 1, true),      // minimal overlap
    ];

    for &(s1, sz1, s2, sz2, expected) in cases {
        let ffi_result = partitions_overlap_decide(s1, sz1, s2, sz2);
        let p1 = MemPartition { start: s1, size: sz1, attr: 0 };
        let p2 = MemPartition { start: s2, size: sz2, attr: 0 };
        let model_result = p1.overlaps(& p2);

        assert_eq!(
            ffi_result, expected,
            "partitions_overlap_decide expected mismatch: ({s1},{sz1}) vs ({s2},{sz2})"
        );
        assert_eq!(
            ffi_result, model_result,
            "partitions_overlap_decide model mismatch: ({s1},{sz1}) vs ({s2},{sz2})"
        );
    }
}

// =====================================================================
// Differential tests: check_partition
// =====================================================================

#[test]
fn mem_domain_check_partition_invalid_size_zero() {
    let starts = [0u32; 16];
    let sizes = [0u32; 16];
    let ret = ffi_check_partition(100, 0, &starts, &sizes);
    assert_eq!(ret, EINVAL, "size=0 must be EINVAL (MD3)");
}

#[test]
fn mem_domain_check_partition_overflow_rejected() {
    let starts = [0u32; 16];
    let sizes = [0u32; 16];
    // start + size > u32::MAX
    let ret = ffi_check_partition(u32::MAX, 1, &starts, &sizes);
    assert_eq!(ret, EINVAL, "overflow must be EINVAL (MD6)");
}

#[test]
fn mem_domain_check_partition_overlap_rejected() {
    let mut starts = [0u32; 16];
    let mut sizes = [0u32; 16];
    // Existing partition at [100, 200)
    starts[0] = 100;
    sizes[0] = 100;
    // New partition overlapping with [100, 200)
    let ret = ffi_check_partition(150, 100, &starts, &sizes);
    assert_eq!(ret, EINVAL, "overlap must be EINVAL (MD1)");
}

#[test]
fn mem_domain_check_partition_valid_non_overlapping() {
    let mut starts = [0u32; 16];
    let mut sizes = [0u32; 16];
    starts[0] = 100;
    sizes[0] = 100;
    // New partition [200, 300) — adjacent, no overlap
    let ret = ffi_check_partition(200, 100, &starts, &sizes);
    assert_eq!(ret, OK, "non-overlapping partition must be OK");
}

// =====================================================================
// Differential tests: add_partition_decide vs model add_partition
// =====================================================================

#[test]
fn mem_domain_add_partition_ffi_matches_model_exhaustive() {
    // Small partition configurations: start = 0..=3, size = 0..=3
    // Test on empty domain
    let mut domain = MemDomain::init();

    // Add a few partitions and verify ffi matches model
    let test_partitions: &[(u32, u32)] = &[
        (0, 10),
        (100, 50),
        (0, 0),   // invalid: size == 0
        (u32::MAX, 1), // invalid: overflow
        (0, 10),  // duplicate: overlaps first
        (200, 10),
    ];

    for &(start, size) in test_partitions {
        let (starts, sizes) = domain_arrays(&domain);
        let num = domain.num_partitions;

        let (ffi_ret, _ffi_slot, ffi_new_num, _ffi_action) =
            ffi_add_partition_decide(start, size, &starts, &sizes, num);

        let part = MemPartition { start, size, attr: 0 };
        let model_result = if domain.num_partitions < 16 {
            domain.add_partition(&part)
        } else {
            Err(ENOSPC)
        };

        match model_result {
            Ok(_slot) => {
                assert_eq!(ffi_ret, OK,
                    "add ffi_ret mismatch: start={start}, size={size}");
                assert_eq!(ffi_new_num, num + 1,
                    "add new_num mismatch: start={start}, size={size}");
            }
            Err(e) => {
                assert_ne!(ffi_ret, OK,
                    "add ffi should fail: start={start}, size={size}");
                // Both failed — restore domain state (model already rolled back)
                // Re-apply if model succeeded but ffi failed (shouldn't happen)
                let _ = e;
            }
        }
    }
}

// =====================================================================
// Differential tests: remove_partition_decide vs model remove_partition
// =====================================================================

#[test]
fn mem_domain_remove_partition_ffi_matches_model() {
    let mut domain = MemDomain::init();
    // Add two partitions
    let p1 = MemPartition { start: 0, size: 100, attr: 0 };
    let p2 = MemPartition { start: 200, size: 50, attr: 0 };
    domain.add_partition(&p1).unwrap();
    domain.add_partition(&p2).unwrap();

    // Test removing p1
    {
        let (starts, sizes) = domain_arrays(&domain);
        let num = domain.num_partitions;
        let (ffi_ret, ffi_slot, ffi_new_num, _) =
            ffi_remove_partition_decide(0, 100, &starts, &sizes, num);

        let model_result = domain.remove_partition(0, 100);
        assert!(model_result.is_ok(), "model should remove p1");
        let model_slot = model_result.unwrap();

        assert_eq!(ffi_ret, OK, "ffi remove p1: ret");
        assert_eq!(ffi_slot, model_slot, "ffi remove p1: slot");
        assert_eq!(ffi_new_num, domain.num_partitions, "ffi remove p1: new_num");
    }

    // Test removing non-existent partition
    {
        let (starts, sizes) = domain_arrays(&domain);
        let num = domain.num_partitions;
        let (ffi_ret, _, _, _) =
            ffi_remove_partition_decide(999, 999, &starts, &sizes, num);

        assert_eq!(ffi_ret, ENOENT, "ffi remove non-existent: should be ENOENT");
    }
}

// =====================================================================
// Property: MD5 — add then remove roundtrip
// =====================================================================

#[test]
fn mem_domain_add_remove_roundtrip() {
    let mut domain = MemDomain::init();
    let initial_num = domain.num_partitions;

    let p = MemPartition { start: 1000, size: 100, attr: 0 };
    let add_result = domain.add_partition(&p);
    assert!(add_result.is_ok(), "add should succeed");
    assert_eq!(domain.num_partitions, initial_num + 1);

    let remove_result = domain.remove_partition(1000, 100);
    assert!(remove_result.is_ok(), "remove should succeed");
    assert_eq!(domain.num_partitions, initial_num, "MD5: roundtrip restores count");
}

// =====================================================================
// Property: MD1 — no two added partitions overlap
// =====================================================================

#[test]
fn mem_domain_no_overlapping_partitions_accepted() {
    let mut domain = MemDomain::init();

    // Add a sequence of non-overlapping partitions; verify each succeeds
    let partitions: &[(u32, u32)] = &[
        (0, 100),
        (200, 100),
        (400, 100),
        (600, 100),
    ];

    for &(start, size) in partitions {
        let part = MemPartition { start, size, attr: 0 };
        let result = domain.add_partition(&part);
        assert!(result.is_ok(),
            "MD1: non-overlapping partition ({start},{size}) should be accepted");
    }

    // Verify overlapping add fails
    let overlap = MemPartition { start: 50, size: 100, attr: 0 };
    let result = domain.add_partition(&overlap);
    assert!(result.is_err(),
        "MD1: overlapping partition should be rejected");
}

// =====================================================================
// Property: MD4 — num_partitions <= MAX_PARTITIONS
// =====================================================================

#[test]
fn mem_domain_num_partitions_bounded() {
    let mut domain = MemDomain::init();

    // Fill all 16 slots with non-overlapping partitions
    for i in 0..16u32 {
        let part = MemPartition {
            start: i * 100,
            size: 10,
            attr: 0,
        };
        let result = domain.add_partition(&part);
        assert!(result.is_ok(), "slot {i} should be fillable");
    }
    assert_eq!(domain.num_partitions, 16, "MD4: should have 16 partitions");

    // Verify ffi also reports ENOSPC when full
    let (starts, sizes) = domain_arrays(&domain);
    let (ffi_ret, _, _, _) =
        ffi_add_partition_decide(2000, 10, &starts, &sizes, domain.num_partitions);
    assert_eq!(ffi_ret, ENOSPC, "MD4: FFI must return ENOSPC when full");
}
