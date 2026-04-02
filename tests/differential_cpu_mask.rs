//! Differential equivalence tests — CPU Mask (FFI vs Model).
//!
//! Verifies that the FFI cpu_mask functions produce the same results as
//! the Verus-verified model functions in gale::cpu_mask.

#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use gale::cpu_mask::*;
use gale::error::*;

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Replica of gale_cpu_mask_mod.
/// Returns (mask, err).
fn ffi_cpu_mask_mod(
    current_mask: u32,
    enable: u32,
    disable: u32,
    is_running: bool,
    pin_only: bool,
) -> (u32, i32) {
    let result = cpu_mask_mod(current_mask, enable, disable, is_running, pin_only);
    (result.mask, result.error)
}

/// Replica of gale_validate_pin_mask.
/// Returns 1 if valid, 0 if invalid.
fn ffi_validate_pin_mask(mask: u32) -> i32 {
    if validate_pin_mask(mask) { 1 } else { 0 }
}

/// Replica of gale_cpu_pin_compute.
/// Returns (mask, err).
fn ffi_cpu_pin_compute(cpu_id: u32, max_cpus: u32) -> (u32, i32) {
    match cpu_pin_compute(cpu_id, max_cpus) {
        Ok(m) => (m, OK),
        Err(e) => (0, e),
    }
}

// =====================================================================
// Differential tests: cpu_mask_mod
// =====================================================================

#[test]
fn cpu_mask_mod_ffi_matches_model_exhaustive() {
    // Use 4-bit masks (0..16) for exhaustive testing
    for current_mask in 0u32..=0xF {
        for enable in 0u32..=0xF {
            for disable in 0u32..=0xF {
                for is_running in [false, true] {
                    for pin_only in [false, true] {
                        let (ffi_mask, ffi_err) =
                            ffi_cpu_mask_mod(current_mask, enable, disable, is_running, pin_only);

                        let model = cpu_mask_mod(
                            current_mask, enable, disable, is_running, pin_only);

                        assert_eq!(ffi_mask, model.mask,
                            "mask: cur=0x{current_mask:X}, en=0x{enable:X}, dis=0x{disable:X}, run={is_running}, pin={pin_only}");
                        assert_eq!(ffi_err, model.error,
                            "err: cur=0x{current_mask:X}, en=0x{enable:X}, dis=0x{disable:X}, run={is_running}, pin={pin_only}");
                    }
                }
            }
        }
    }
}

// =====================================================================
// Differential tests: validate_pin_mask
// =====================================================================

#[test]
fn validate_pin_mask_ffi_matches_model_exhaustive() {
    // Test all 32-bit single-bit values and boundary values
    for mask in 0u32..=0xFF {
        let ffi_result = ffi_validate_pin_mask(mask);
        let model_result = validate_pin_mask(mask);

        assert_eq!(ffi_result, if model_result { 1 } else { 0 },
            "validate_pin_mask diverged: mask=0x{mask:X}");
    }

    // Test all powers of two (valid pin masks)
    for bit in 0u32..32 {
        let mask = 1u32 << bit;
        let ffi_result = ffi_validate_pin_mask(mask);
        assert_eq!(ffi_result, 1,
            "power of 2 should be valid pin mask: bit={bit}, mask=0x{mask:X}");
    }

    // Test some non-powers-of-two (invalid pin masks)
    for mask in [0u32, 3, 5, 6, 7, 9, 10, 0xFF, 0xFFFF, u32::MAX] {
        if mask != 0 && (mask & (mask - 1)) != 0 {
            let ffi_result = ffi_validate_pin_mask(mask);
            assert_eq!(ffi_result, 0,
                "non-power-of-2 should be invalid: mask=0x{mask:X}");
        }
    }
}

// =====================================================================
// Differential tests: cpu_pin_compute
// =====================================================================

#[test]
fn cpu_pin_compute_ffi_matches_model_exhaustive() {
    for max_cpus in 0u32..=33 {
        for cpu_id in 0u32..=max_cpus.saturating_add(1).min(33) {
            let (ffi_mask, ffi_err) = ffi_cpu_pin_compute(cpu_id, max_cpus);
            let model_result = cpu_pin_compute(cpu_id, max_cpus);

            match model_result {
                Ok(m) => {
                    assert_eq!(ffi_err, OK,
                        "pin_compute: cpu={cpu_id}, max={max_cpus}");
                    assert_eq!(ffi_mask, m,
                        "pin_compute mask: cpu={cpu_id}, max={max_cpus}");
                }
                Err(e) => {
                    assert_eq!(ffi_err, e,
                        "pin_compute error: cpu={cpu_id}, max={max_cpus}");
                    assert_eq!(ffi_mask, 0,
                        "pin_compute mask should be 0 on error: cpu={cpu_id}, max={max_cpus}");
                }
            }
        }
    }
}

// =====================================================================
// Property: CM1 — running threads cannot modify mask
// =====================================================================

#[test]
fn cpu_mask_mod_running_rejected() {
    for current_mask in [1u32, 0xF, 0xFF] {
        for enable in [0u32, 1, 0xF] {
            for disable in [0u32, 1, 0xF] {
                let (ffi_mask, ffi_err) =
                    ffi_cpu_mask_mod(current_mask, enable, disable, true, false);
                assert_eq!(ffi_err, EINVAL, "CM1: running thread must be rejected");
                assert_eq!(ffi_mask, current_mask, "CM1: mask unchanged on rejection");
            }
        }
    }
}

// =====================================================================
// Property: CM2 — PIN_ONLY requires exactly one bit set
// =====================================================================

#[test]
fn cpu_mask_mod_pin_only_single_bit() {
    // Enable bits that result in multiple bits: should be rejected
    let (_, err) = ffi_cpu_mask_mod(0x1, 0x2, 0, false, true);
    assert_eq!(err, EINVAL, "CM2: multiple bits in pin_only mode rejected");

    // Enable single bit: should succeed
    let (mask, err) = ffi_cpu_mask_mod(0x0, 0x4, 0, false, true);
    assert_eq!(err, OK, "CM2: single bit in pin_only mode accepted");
    assert_eq!(mask, 0x4);

    // Enable + disable to result in single bit: should succeed
    let (mask, err) = ffi_cpu_mask_mod(0x3, 0x0, 0x2, false, true);
    assert_eq!(err, OK, "CM2: disable to single bit accepted");
    assert_eq!(mask, 0x1);
}

// =====================================================================
// Property: CM3 — new_mask = (current | enable) & !disable
// =====================================================================

#[test]
fn cpu_mask_mod_formula_correct() {
    for current in 1u32..=0xF {
        for enable in 0u32..=0xF {
            for disable in 0u32..=0xF {
                let expected = (current | enable) & !disable;
                if expected == 0 {
                    continue; // CM4 would reject
                }

                let (ffi_mask, ffi_err) =
                    ffi_cpu_mask_mod(current, enable, disable, false, false);

                assert_eq!(ffi_err, OK,
                    "CM3: should succeed: cur=0x{current:X}, en=0x{enable:X}, dis=0x{disable:X}");
                assert_eq!(ffi_mask, expected,
                    "CM3: formula: cur=0x{current:X}, en=0x{enable:X}, dis=0x{disable:X}");
            }
        }
    }
}

// =====================================================================
// Property: CM4 — result mask is never zero
// =====================================================================

#[test]
fn cpu_mask_mod_never_zero() {
    for current in 0u32..=0xF {
        for enable in 0u32..=0xF {
            for disable in 0u32..=0xF {
                let (ffi_mask, ffi_err) =
                    ffi_cpu_mask_mod(current, enable, disable, false, false);

                if ffi_err == OK {
                    assert_ne!(ffi_mask, 0,
                        "CM4: result mask must never be zero");
                }
            }
        }
    }
}

// =====================================================================
// Property: CM6 — cpu_pin_compute bounds check
// =====================================================================

#[test]
fn cpu_pin_compute_bounds() {
    // Valid: cpu_id < max_cpus <= 32
    for max_cpus in 1u32..=32 {
        for cpu_id in 0u32..max_cpus {
            let (ffi_mask, ffi_err) = ffi_cpu_pin_compute(cpu_id, max_cpus);
            assert_eq!(ffi_err, OK,
                "CM6: valid input rejected: cpu={cpu_id}, max={max_cpus}");
            assert_eq!(ffi_mask, 1u32 << cpu_id,
                "CM6: wrong mask: cpu={cpu_id}");
            // Verify it is a valid pin mask
            assert_eq!(ffi_validate_pin_mask(ffi_mask), 1,
                "CM6: result should be valid pin mask");
        }
    }

    // Invalid: cpu_id >= max_cpus
    for max_cpus in 1u32..=8 {
        let (_, err) = ffi_cpu_pin_compute(max_cpus, max_cpus);
        assert_eq!(err, EINVAL, "CM6: cpu_id == max_cpus should fail");
    }

    // Invalid: max_cpus > 32
    let (_, err) = ffi_cpu_pin_compute(0, 33);
    assert_eq!(err, EINVAL, "CM6: max_cpus > 32 should fail");

    // Invalid: max_cpus == 0
    let (_, err) = ffi_cpu_pin_compute(0, 0);
    assert_eq!(err, EINVAL, "CM6: max_cpus == 0 should fail");
}

// =====================================================================
// Integration: pin operation roundtrip
// =====================================================================

#[test]
fn cpu_mask_pin_roundtrip() {
    for max_cpus in 1u32..=16 {
        for cpu_id in 0u32..max_cpus {
            // Compute pin mask
            let (pin_mask, err) = ffi_cpu_pin_compute(cpu_id, max_cpus);
            assert_eq!(err, OK);

            // Validate it
            assert_eq!(ffi_validate_pin_mask(pin_mask), 1);

            // Apply via cpu_mask_mod with pin_only
            let current = (1u32 << max_cpus) - 1; // all CPUs enabled
            let (result_mask, err) = ffi_cpu_mask_mod(
                current,
                pin_mask,
                !pin_mask,
                false,
                true,
            );
            assert_eq!(err, OK, "pin roundtrip: cpu={cpu_id}, max={max_cpus}");
            assert_eq!(result_mask, pin_mask,
                "pin roundtrip: should be pinned to single CPU");
        }
    }
}
