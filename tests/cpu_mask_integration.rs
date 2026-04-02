//! Integration tests for the CPU affinity mask model.
//!
//! These tests run under: cargo test, miri, sanitizers.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::shadow_unrelated
)]

use gale::cpu_mask::{cpu_mask_mod, cpu_pin_compute, validate_pin_mask, MAX_CPUS};
use gale::error::*;

// ==========================================================================
// CM1: Running threads cannot have mask modified
// ==========================================================================

#[test]
fn running_thread_returns_einval() {
    let result = cpu_mask_mod(0xFF, 0x01, 0x00, true, false);
    assert_eq!(result.error, EINVAL);
}

#[test]
fn running_thread_returns_einval_pin_only() {
    let result = cpu_mask_mod(0x01, 0x02, 0x01, true, true);
    assert_eq!(result.error, EINVAL);
}

#[test]
fn running_thread_preserves_current_mask() {
    let result = cpu_mask_mod(0xAB, 0xFF, 0x00, true, false);
    assert_eq!(result.error, EINVAL);
    assert_eq!(result.mask, 0xAB);
}

// ==========================================================================
// CM2: PIN_ONLY rejects multi-bit masks
// ==========================================================================

#[test]
fn pin_only_accepts_single_bit() {
    let result = cpu_mask_mod(0x00, 0x04, 0x00, false, true);
    assert_eq!(result.error, OK);
    assert_eq!(result.mask, 0x04);
}

#[test]
fn pin_only_rejects_two_bits() {
    // enable bits 0 and 1 => mask = 0x03 which has two bits set
    let result = cpu_mask_mod(0x00, 0x03, 0x00, false, true);
    assert_eq!(result.error, EINVAL);
}

#[test]
fn pin_only_rejects_many_bits() {
    let result = cpu_mask_mod(0x00, 0xFF, 0x00, false, true);
    assert_eq!(result.error, EINVAL);
}

#[test]
fn pin_only_accepts_bit_15() {
    let result = cpu_mask_mod(0x00, 1 << 15, 0x00, false, true);
    assert_eq!(result.error, OK);
    assert_eq!(result.mask, 1 << 15);
}

// ==========================================================================
// CM3: New mask = (current | enable) & !disable
// ==========================================================================

#[test]
fn enable_single_bit() {
    let result = cpu_mask_mod(0x01, 0x02, 0x00, false, false);
    assert_eq!(result.error, OK);
    assert_eq!(result.mask, 0x03);
}

#[test]
fn disable_single_bit() {
    let result = cpu_mask_mod(0x03, 0x00, 0x02, false, false);
    assert_eq!(result.error, OK);
    assert_eq!(result.mask, 0x01);
}

#[test]
fn enable_and_disable_simultaneously() {
    // current=0x0F, enable bit 4, disable bit 0 => 0x1E
    let result = cpu_mask_mod(0x0F, 0x10, 0x01, false, false);
    assert_eq!(result.error, OK);
    assert_eq!(result.mask, 0x1E);
}

#[test]
fn enable_already_set_bit_is_idempotent() {
    let result = cpu_mask_mod(0x05, 0x01, 0x00, false, false);
    assert_eq!(result.error, OK);
    assert_eq!(result.mask, 0x05);
}

#[test]
fn disable_already_clear_bit_is_idempotent() {
    let result = cpu_mask_mod(0x05, 0x00, 0x02, false, false);
    assert_eq!(result.error, OK);
    assert_eq!(result.mask, 0x05);
}

// ==========================================================================
// CM4: Result mask is never zero (at least one CPU)
// ==========================================================================

#[test]
fn cannot_clear_all_cpus() {
    // Disable all bits: mask would become 0 => EINVAL
    let result = cpu_mask_mod(0xFF, 0x00, 0xFFFFFFFF, false, false);
    assert_eq!(result.error, EINVAL);
}

#[test]
fn cannot_produce_zero_mask() {
    let result = cpu_mask_mod(0x01, 0x00, 0x01, false, false);
    assert_eq!(result.error, EINVAL);
}

// ==========================================================================
// Edge cases: all CPUs enabled, single CPU, CPU 0, max CPU
// ==========================================================================

#[test]
fn enable_all_cpus() {
    let result = cpu_mask_mod(0x00, 0xFFFFFFFF, 0x00, false, false);
    assert_eq!(result.error, OK);
    assert_eq!(result.mask, 0xFFFFFFFF);
}

#[test]
fn single_cpu_enable_disable_cycle() {
    // Start with CPU 0, switch to CPU 1
    let r1 = cpu_mask_mod(0x01, 0x02, 0x01, false, false);
    assert_eq!(r1.error, OK);
    assert_eq!(r1.mask, 0x02);

    // Switch back
    let r2 = cpu_mask_mod(r1.mask, 0x01, 0x02, false, false);
    assert_eq!(r2.error, OK);
    assert_eq!(r2.mask, 0x01);
}

#[test]
fn pin_to_cpu_0() {
    // Models k_thread_cpu_pin(thread, 0): enable=BIT(0), disable=~BIT(0)
    let result = cpu_mask_mod(0xFF, 0x01, !0x01u32, false, true);
    assert_eq!(result.error, OK);
    assert_eq!(result.mask, 0x01);
}

#[test]
fn pin_to_max_cpu() {
    // Pin to CPU 15 (highest in 16-CPU system)
    let bit15 = 1u32 << 15;
    let result = cpu_mask_mod(0xFF, bit15, !bit15, false, true);
    assert_eq!(result.error, OK);
    assert_eq!(result.mask, bit15);
}

// ==========================================================================
// validate_pin_mask
// ==========================================================================

#[test]
fn validate_pin_mask_zero_is_invalid() {
    assert!(!validate_pin_mask(0));
}

#[test]
fn validate_pin_mask_single_bits() {
    for i in 0..32u32 {
        assert!(validate_pin_mask(1 << i), "bit {} should be valid", i);
    }
}

#[test]
fn validate_pin_mask_two_bits_invalid() {
    assert!(!validate_pin_mask(0x03));
    assert!(!validate_pin_mask(0x05));
    assert!(!validate_pin_mask(0xFF));
    assert!(!validate_pin_mask(0xFFFFFFFF));
}

#[test]
fn validate_pin_mask_powers_of_two() {
    let mut v = 1u32;
    while v != 0 {
        assert!(validate_pin_mask(v));
        v = v.wrapping_shl(1);
    }
}

// ==========================================================================
// cpu_pin_compute
// ==========================================================================

#[test]
fn cpu_pin_compute_valid_ids() {
    for cpu in 0..16u32 {
        let result = cpu_pin_compute(cpu, MAX_CPUS);
        assert!(result.is_ok());
        let mask = result.unwrap();
        assert_eq!(mask, 1 << cpu);
        assert!(validate_pin_mask(mask));
    }
}

#[test]
fn cpu_pin_compute_cpu_0() {
    let result = cpu_pin_compute(0, 4);
    assert_eq!(result, Ok(1));
}

#[test]
fn cpu_pin_compute_max_cpu_31() {
    let result = cpu_pin_compute(31, 32);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 1u32 << 31);
}

#[test]
fn cpu_pin_compute_out_of_bounds() {
    assert_eq!(cpu_pin_compute(16, 16), Err(EINVAL));
    assert_eq!(cpu_pin_compute(4, 4), Err(EINVAL));
    assert_eq!(cpu_pin_compute(100, 16), Err(EINVAL));
}

#[test]
fn cpu_pin_compute_max_cpus_too_large() {
    assert_eq!(cpu_pin_compute(0, 33), Err(EINVAL));
    assert_eq!(cpu_pin_compute(0, u32::MAX), Err(EINVAL));
}

#[test]
fn cpu_pin_compute_max_cpus_zero() {
    // cpu_id >= max_cpus always true when max_cpus == 0
    assert_eq!(cpu_pin_compute(0, 0), Err(EINVAL));
}

// ==========================================================================
// Overflow: max u32 values
// ==========================================================================

#[test]
fn enable_max_u32() {
    let result = cpu_mask_mod(0x00, u32::MAX, 0x00, false, false);
    assert_eq!(result.error, OK);
    assert_eq!(result.mask, u32::MAX);
}

#[test]
fn disable_max_u32_from_full() {
    // Disabling all bits from a full mask => zero mask => EINVAL
    let result = cpu_mask_mod(u32::MAX, 0x00, u32::MAX, false, false);
    assert_eq!(result.error, EINVAL);
}

#[test]
fn enable_and_disable_max_u32() {
    // enable all then disable all: (0 | 0xFFFFFFFF) & !0xFFFFFFFF == 0 => EINVAL
    let result = cpu_mask_mod(0x00, u32::MAX, u32::MAX, false, false);
    assert_eq!(result.error, EINVAL);
}

#[test]
fn current_mask_max_u32_no_change() {
    let result = cpu_mask_mod(u32::MAX, 0x00, 0x00, false, false);
    assert_eq!(result.error, OK);
    assert_eq!(result.mask, u32::MAX);
}

// ==========================================================================
// Full workflow: k_thread_cpu_pin equivalent
// ==========================================================================

#[test]
fn full_pin_workflow() {
    // 1. Compute pin mask for CPU 3
    let pin_mask = cpu_pin_compute(3, 16).unwrap();
    assert_eq!(pin_mask, 0x08);

    // 2. Apply: enable=pin_mask, disable=!pin_mask (like k_thread_cpu_pin)
    let result = cpu_mask_mod(0xFFFF, pin_mask, !pin_mask, false, true);
    assert_eq!(result.error, OK);
    assert_eq!(result.mask, 0x08);
    assert!(validate_pin_mask(result.mask));
}
