//! Differential equivalence tests — IPI (FFI vs Model).
//!
//! Verifies that the FFI IPI mask functions produce the same results as
//! the Verus-verified model functions in gale::ipi.

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

use gale::ipi::{compute_ipi_mask, validate_ipi_mask};

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Replica of gale_compute_ipi_mask.
///
/// Accepts slices rather than raw pointers (no unsafe needed in tests).
fn ffi_compute_ipi_mask(
    current_cpu: u32,
    target_prio: i32,
    target_cpu_mask: u32,
    cpu_prios: &[i32],
    cpu_active: &[u8],
    num_cpus: u32,
    max_cpus: u32,
) -> u32 {
    if cpu_prios.is_empty() || cpu_active.is_empty() {
        return 0;
    }
    if num_cpus == 0 || current_cpu >= num_cpus || num_cpus > max_cpus || max_cpus > 16 {
        return 0;
    }

    // Convert u8 active array to bool
    let mut active_bool = [false; 16];
    let mut i: usize = 0;
    while i < num_cpus as usize {
        active_bool[i] = cpu_active[i] != 0;
        i += 1;
    }
    let active = &active_bool[..num_cpus as usize];

    compute_ipi_mask(current_cpu, target_prio, target_cpu_mask, cpu_prios, active, num_cpus, max_cpus)
}

/// Replica of gale_validate_ipi_mask.
fn ffi_validate_ipi_mask(mask: u32, current_cpu: u32, max_cpus: u32) -> i32 {
    if current_cpu >= max_cpus || max_cpus > 16 {
        return 0;
    }
    if validate_ipi_mask(mask, current_cpu, max_cpus) {
        1
    } else {
        0
    }
}

// =====================================================================
// Helpers
// =====================================================================

/// Check if bit `i` is set in `mask`.
fn bit_set(mask: u32, i: u32) -> bool {
    mask & (1u32 << i) != 0u32
}

// =====================================================================
// Differential tests: compute_ipi_mask
// =====================================================================

#[test]
fn ipi_mask_empty_when_null_or_invalid() {
    // num_cpus=0 => 0
    let prios = [0i32; 4];
    let active = [1u8; 4];
    let result = ffi_compute_ipi_mask(0, 0, 0xFFFF, &prios, &active, 0, 4);
    assert_eq!(result, 0, "num_cpus=0 => mask=0");

    // current_cpu >= num_cpus => 0
    let result = ffi_compute_ipi_mask(4, 0, 0xFFFF, &prios, &active, 4, 4);
    assert_eq!(result, 0, "current_cpu >= num_cpus => mask=0");

    // max_cpus > 16 => 0
    let result = ffi_compute_ipi_mask(0, 0, 0xFFFF, &prios, &active, 4, 17);
    assert_eq!(result, 0, "max_cpus > 16 => mask=0");
}

#[test]
fn ipi_mask_single_cpu_zero() {
    // Only 1 CPU (CPU 0 = current) — no one to IPI
    let prios = [5i32];
    let active = [1u8];
    let result = ffi_compute_ipi_mask(0, 0, 0xFFFF, &prios, &active, 1, 1);
    assert_eq!(result, 0, "IP1: single CPU must produce empty mask");
}

#[test]
fn ipi_mask_current_cpu_never_included() {
    // With 4 CPUs, current is always excluded
    let prios = [10i32, 10, 10, 10];
    let active = [1u8, 1, 1, 1];

    for current in 0u32..4 {
        let mask = ffi_compute_ipi_mask(
            current, -100, 0xFFFF, &prios, &active, 4, 4,
        );
        assert!(!bit_set(mask, current),
            "IP1: current CPU {current} must not be in mask={mask:#010x}");
    }
}

#[test]
fn ipi_mask_only_includes_cpus_in_bounds() {
    let prios = [100i32, 100, 100, 100];
    let active = [1u8, 1, 1, 1];

    let mask = ffi_compute_ipi_mask(0, -200, 0xFFFF, &prios, &active, 4, 4);

    // No bits >= num_cpus (4) should be set
    for i in 4u32..32 {
        assert!(!bit_set(mask, i),
            "IP2: bit {i} must not be set in mask={mask:#010x}");
    }
}

#[test]
fn ipi_mask_respects_target_cpu_mask() {
    // Allow only CPU 2 (bit 2 set) in target_cpu_mask
    let prios = [10i32, 10, 10, 10];
    let active = [1u8, 1, 1, 1];
    let target_cpu_mask = 0b0100u32; // only CPU 2

    let mask = ffi_compute_ipi_mask(0, -100, target_cpu_mask, &prios, &active, 4, 4);

    // CPU 1 and CPU 3 must not be set
    assert!(!bit_set(mask, 1), "IP3: CPU 1 not in affinity mask must be excluded");
    assert!(!bit_set(mask, 3), "IP3: CPU 3 not in affinity mask must be excluded");
    // CPU 2 should be set (prio 10 > -100)
    assert!(bit_set(mask, 2), "IP3: CPU 2 in affinity mask with higher prio must be included");
}

#[test]
fn ipi_mask_priority_gating() {
    // CPU 1: prio 5 (lower priority — 5 > 10 is false, so NOT included)
    // CPU 2: prio 20 (higher priority number — 20 > 10, so included)
    // CPU 3: prio 10 == target_prio — NOT included (must be strictly greater)
    let prios = [0i32, 5, 20, 10];
    let active = [1u8, 1, 1, 1];
    let target_prio = 10i32;

    let mask = ffi_compute_ipi_mask(0, target_prio, 0xFFFF, &prios, &active, 4, 4);

    assert!(!bit_set(mask, 1), "IP4: CPU 1 prio 5 <= 10 must not be included");
    assert!(bit_set(mask, 2), "IP4: CPU 2 prio 20 > 10 must be included");
    assert!(!bit_set(mask, 3), "IP4: CPU 3 prio 10 == 10 must not be included (not strictly greater)");
}

#[test]
fn ipi_mask_inactive_cpus_excluded() {
    // CPU 1: active, CPU 2: inactive, CPU 3: active
    let prios = [0i32, 100, 100, 100];
    let active = [1u8, 1, 0, 1]; // CPU 2 inactive
    let target_prio = -1000i32;   // very low priority => all eligible by prio

    let mask = ffi_compute_ipi_mask(0, target_prio, 0xFFFF, &prios, &active, 4, 4);

    assert!(bit_set(mask, 1), "active CPU 1 must be included");
    assert!(!bit_set(mask, 2), "inactive CPU 2 must be excluded");
    assert!(bit_set(mask, 3), "active CPU 3 must be included");
}

#[test]
fn ipi_mask_all_inactive_is_zero() {
    let prios = [0i32, 100, 100, 100];
    let active = [1u8, 0, 0, 0]; // only current CPU is "active"
    let target_prio = -1000i32;

    let mask = ffi_compute_ipi_mask(0, target_prio, 0xFFFF, &prios, &active, 4, 4);
    assert_eq!(mask, 0, "all non-current CPUs inactive => empty mask");
}

#[test]
fn ipi_mask_ffi_matches_model_exhaustive_2cpu() {
    // Exhaustive test with 2 CPUs
    for current in 0u32..2 {
        for target_prio in [-10i32, 0, 5, 10] {
            for cpu0_prio in [-5i32, 0, 5, 15] {
                for cpu1_prio in [-5i32, 0, 5, 15] {
                    for cpu0_active in [0u8, 1] {
                        for cpu1_active in [0u8, 1] {
                            for target_mask in [0u32, 1, 2, 3] {
                                let prios = [cpu0_prio, cpu1_prio];
                                let active = [cpu0_active, cpu1_active];

                                if current >= 2 {
                                    continue;
                                }

                                let ffi_mask = ffi_compute_ipi_mask(
                                    current, target_prio, target_mask,
                                    &prios, &active, 2, 4,
                                );

                                // Model call
                                let active_bool = [cpu0_active != 0, cpu1_active != 0];
                                let model_mask = compute_ipi_mask(
                                    current, target_prio, target_mask,
                                    &prios, &active_bool, 2, 4,
                                );

                                assert_eq!(ffi_mask, model_mask,
                                    "mask mismatch: current={current}, tprio={target_prio}, tmask={target_mask:#x}");

                                // IP1: current never set
                                assert!(!bit_set(ffi_mask, current),
                                    "IP1: current CPU must not be set");
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn ipi_mask_ffi_matches_model_exhaustive_4cpu() {
    // Wider test with 4 CPUs, varied active patterns and priorities
    let prio_values = [-10i32, 0, 10, 20];
    let active_patterns: &[[u8; 4]] = &[
        [1, 1, 1, 1],
        [1, 0, 1, 0],
        [1, 1, 0, 0],
        [1, 0, 0, 1],
    ];

    for current in 0u32..4 {
        for target_prio in [0i32, 10, 15] {
            for active_pat in active_patterns {
                for target_mask in [0u32, 0xF, 0b0101, 0b1010] {
                    let prios = prio_values;
                    let ffi_mask = ffi_compute_ipi_mask(
                        current, target_prio, target_mask,
                        &prios, active_pat.as_ref(), 4, 4,
                    );

                    let active_bool = [
                        active_pat[0] != 0,
                        active_pat[1] != 0,
                        active_pat[2] != 0,
                        active_pat[3] != 0,
                    ];
                    let model_mask = compute_ipi_mask(
                        current, target_prio, target_mask,
                        &prios, &active_bool, 4, 4,
                    );

                    assert_eq!(ffi_mask, model_mask,
                        "4cpu mismatch: current={current}, tprio={target_prio}, tmask={target_mask:#x}");

                    // IP1: current never set
                    assert!(!bit_set(ffi_mask, current),
                        "IP1: current CPU {current} must not be set in mask={ffi_mask:#010x}");

                    // IP5: no bits >= num_cpus (4)
                    for i in 4u32..32 {
                        assert!(!bit_set(ffi_mask, i),
                            "IP5: bit {i} must not be set");
                    }
                }
            }
        }
    }
}

// =====================================================================
// Differential tests: validate_ipi_mask
// =====================================================================

#[test]
fn ipi_validate_ffi_matches_model_exhaustive() {
    for current_cpu in 0u32..4 {
        for max_cpus in (current_cpu + 1)..=4 {
            for mask in [0u32, 1, 2, 3, 4, 7, 8, 0xF, 0xFF] {
                let ffi_result = ffi_validate_ipi_mask(mask, current_cpu, max_cpus);
                let model_result = validate_ipi_mask(mask, current_cpu, max_cpus);

                let expected = if model_result { 1 } else { 0 };
                assert_eq!(ffi_result, expected,
                    "validate mismatch: mask={mask:#x}, cpu={current_cpu}, max={max_cpus}");
            }
        }
    }
}

#[test]
fn ipi_validate_rejects_current_cpu_bit() {
    // A mask with the current CPU bit set must be rejected
    for current in 0u32..4 {
        let mask_with_current = 1u32 << current;
        let result = ffi_validate_ipi_mask(mask_with_current, current, 4);
        assert_eq!(result, 0,
            "IP1: mask with current CPU bit must be invalid: cpu={current}");
    }
}

#[test]
fn ipi_validate_rejects_out_of_range_bits() {
    // Bits above max_cpus-1 must cause rejection
    let max_cpus = 4u32;
    for high_bit in 4u32..8 {
        let mask = 1u32 << high_bit;
        let result = ffi_validate_ipi_mask(mask, 0, max_cpus);
        assert_eq!(result, 0,
            "IP5: bit {high_bit} above max_cpus={max_cpus} must be rejected");
    }
}

#[test]
fn ipi_validate_accepts_valid_mask() {
    // Mask with only non-current CPUs within range
    // current=0, max=4: bits 1,2,3 are valid
    let valid_mask = 0b1110u32; // bits 1,2,3
    let result = ffi_validate_ipi_mask(valid_mask, 0, 4);
    assert_eq!(result, 1, "valid mask should pass: mask={valid_mask:#x}");

    // Empty mask is always valid
    let result = ffi_validate_ipi_mask(0, 0, 4);
    assert_eq!(result, 1, "empty mask is always valid");
}

#[test]
fn ipi_validate_invalid_args() {
    // current_cpu >= max_cpus => 0
    let result = ffi_validate_ipi_mask(0, 4, 4);
    assert_eq!(result, 0, "current >= max_cpus must be rejected");

    // max_cpus > 16 => 0
    let result = ffi_validate_ipi_mask(0, 0, 17);
    assert_eq!(result, 0, "max_cpus > 16 must be rejected");
}

// =====================================================================
// Property: IP2 — result bounded by num_cpus
// =====================================================================

#[test]
fn ipi_mask_bounded_by_num_cpus() {
    for num_cpus in 1u32..=8 {
        for current in 0..num_cpus {
            let prios: Vec<i32> = (0..num_cpus).map(|_| 100).collect();
            let active: Vec<u8> = (0..num_cpus).map(|_| 1).collect();

            let mask = ffi_compute_ipi_mask(
                current, -200, 0xFFFF, &prios, &active, num_cpus, num_cpus,
            );

            // No bits >= num_cpus should be set
            for i in num_cpus..32 {
                assert!(!bit_set(mask, i),
                    "IP2/IP5: bit {i} must not be set for num_cpus={num_cpus}");
            }
        }
    }
}
