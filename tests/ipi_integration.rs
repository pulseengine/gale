//! Integration tests for the IPI mask creation model.
//!
//! These tests run under: cargo test, miri, sanitizers.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use gale::ipi::*;

// ==========================================================================
// IP1: current CPU is never in the result mask
// ==========================================================================

#[test]
fn current_cpu_excluded_two_cpus() {
    // CPU 0 is current, CPU 1 has lower priority (higher number).
    let mask = compute_ipi_mask(
        0,           // current_cpu
        5,           // target_prio (higher importance)
        0xFFFF_FFFF, // target_cpu_mask (all CPUs allowed)
        &[0, 10],    // cpu_prios
        &[true, true],
        2,           // num_cpus
        2,           // max_cpus
    );
    // CPU 0 must not be set, even though its prio (0) < target (5)
    // would normally not qualify anyway.
    assert_eq!(mask & (1 << 0), 0, "current CPU must be excluded");
}

#[test]
fn current_cpu_excluded_even_if_lower_prio() {
    // CPU 1 is current with priority 100 (very low importance).
    // Target prio is 1 (high importance).
    let mask = compute_ipi_mask(
        1,           // current_cpu
        1,           // target_prio
        0xFFFF_FFFF, // target_cpu_mask
        &[50, 100],  // cpu_prios
        &[true, true],
        2,
        2,
    );
    assert_eq!(mask & (1 << 1), 0, "current CPU must be excluded");
    // CPU 0 has prio 50 > target 1, so it should be in the mask.
    assert_ne!(mask & (1 << 0), 0, "CPU 0 should be targeted");
}

// ==========================================================================
// IP2: only CPUs within [0, num_cpus) can be in the mask
// ==========================================================================

#[test]
fn no_bits_above_num_cpus() {
    let mask = compute_ipi_mask(
        0,
        0,
        0xFFFF_FFFF,
        &[0, 10, 20, 30],
        &[true, true, true, true],
        4,
        16,
    );
    // Bits 4..31 must all be zero.
    assert_eq!(mask >> 4, 0, "no bits above num_cpus");
}

// ==========================================================================
// Single CPU system (result always 0)
// ==========================================================================

#[test]
fn single_cpu_always_zero() {
    // Only one CPU — no one to send an IPI to.
    for prio in [-10, 0, 5, 100] {
        let mask = compute_ipi_mask(
            0,
            prio,
            0xFFFF_FFFF,
            &[50],
            &[true],
            1,
            1,
        );
        assert_eq!(mask, 0, "single CPU system must always produce 0");
    }
}

// ==========================================================================
// Two CPUs: higher/lower priority scenarios
// ==========================================================================

#[test]
fn two_cpus_target_higher_priority() {
    // CPU 0 is current, CPU 1 has prio 10 (low importance).
    // Target prio 5 < CPU 1's prio 10 => CPU 1 should be preempted.
    let mask = compute_ipi_mask(
        0,
        5,
        0xFFFF_FFFF,
        &[0, 10],
        &[true, true],
        2,
        2,
    );
    assert_ne!(mask & (1 << 1), 0, "CPU 1 should get IPI");
}

#[test]
fn two_cpus_target_lower_priority() {
    // CPU 0 is current, CPU 1 has prio 3 (high importance).
    // Target prio 5 > CPU 1's prio 3 => CPU 1 should NOT be preempted.
    let mask = compute_ipi_mask(
        0,
        5,
        0xFFFF_FFFF,
        &[0, 3],
        &[true, true],
        2,
        2,
    );
    assert_eq!(mask & (1 << 1), 0, "CPU 1 should not get IPI");
}

#[test]
fn two_cpus_equal_priority() {
    // Equal priority: cpu_prios[1] == target_prio => no preemption.
    let mask = compute_ipi_mask(
        0,
        5,
        0xFFFF_FFFF,
        &[0, 5],
        &[true, true],
        2,
        2,
    );
    assert_eq!(mask & (1 << 1), 0, "equal prio should not trigger IPI");
}

// ==========================================================================
// CPU mask filtering (thread pinned to specific CPUs)
// ==========================================================================

#[test]
fn cpu_mask_filters_eligible_cpus() {
    // 4 CPUs, all with low-importance prio (high number).
    // Target thread is pinned to CPUs 0 and 2 only (mask = 0b0101).
    let mask = compute_ipi_mask(
        0,       // current_cpu
        1,       // target_prio
        0b0101,  // target_cpu_mask: only CPUs 0, 2
        &[0, 10, 10, 10],
        &[true, true, true, true],
        4,
        4,
    );
    // CPU 0 is current => excluded.
    // CPU 1 is not in cpu_mask => excluded.
    // CPU 2 is in cpu_mask, prio 10 > 1 => included.
    // CPU 3 is not in cpu_mask => excluded.
    assert_eq!(mask, 1 << 2, "only CPU 2 should be in mask");
}

#[test]
fn empty_cpu_mask_yields_zero() {
    // Thread cannot run on any CPU (degenerate case).
    let mask = compute_ipi_mask(
        0,
        1,
        0x0, // no CPUs allowed
        &[0, 10, 10, 10],
        &[true, true, true, true],
        4,
        4,
    );
    assert_eq!(mask, 0, "empty cpu_mask should yield zero");
}

// ==========================================================================
// Inactive CPUs
// ==========================================================================

#[test]
fn inactive_cpus_excluded() {
    // CPU 1 is inactive — should not receive IPI even with lower prio.
    let mask = compute_ipi_mask(
        0,
        1,
        0xFFFF_FFFF,
        &[0, 100],
        &[true, false], // CPU 1 inactive
        2,
        2,
    );
    assert_eq!(mask, 0, "inactive CPU should not get IPI");
}

#[test]
fn empty_active_set() {
    // Only current CPU is active; all others inactive.
    let mask = compute_ipi_mask(
        0,
        1,
        0xFFFF_FFFF,
        &[0, 100, 100, 100],
        &[true, false, false, false],
        4,
        4,
    );
    assert_eq!(mask, 0, "no active remote CPUs => zero mask");
}

// ==========================================================================
// All CPUs same priority
// ==========================================================================

#[test]
fn all_same_priority_no_ipi() {
    // All CPUs have prio 5, target prio is also 5.
    // cpu_prios[i] > target_prio is false for all => no IPIs.
    let mask = compute_ipi_mask(
        0,
        5,
        0xFFFF_FFFF,
        &[5, 5, 5, 5],
        &[true, true, true, true],
        4,
        4,
    );
    assert_eq!(mask, 0, "same priority should not trigger IPIs");
}

#[test]
fn all_same_but_lower_priority_than_target() {
    // All CPUs have prio 10, target prio 5 (higher importance).
    // cpu_prios[i] (10) > target_prio (5) => all remote CPUs get IPI.
    let mask = compute_ipi_mask(
        0,
        5,
        0xFFFF_FFFF,
        &[10, 10, 10, 10],
        &[true, true, true, true],
        4,
        4,
    );
    // CPUs 1, 2, 3 should be set (not CPU 0, which is current).
    assert_eq!(mask, 0b1110, "all remote CPUs should get IPI");
}

// ==========================================================================
// Max CPUs (16)
// ==========================================================================

#[test]
fn max_cpus_all_eligible() {
    let mut prios = [100i32; 16];
    prios[0] = 0; // current CPU's prio doesn't matter
    let active = [true; 16];
    let mask = compute_ipi_mask(
        0,
        1,
        0xFFFF_FFFF,
        &prios,
        &active,
        16,
        16,
    );
    // All CPUs except 0 should be in the mask.
    assert_eq!(mask, 0xFFFE, "all remote CPUs at max config");
}

#[test]
fn max_cpus_none_eligible() {
    // All CPUs have higher importance than target.
    let prios = [0i32; 16];
    let active = [true; 16];
    let mask = compute_ipi_mask(
        0,
        5,
        0xFFFF_FFFF,
        &prios,
        &active,
        16,
        16,
    );
    assert_eq!(mask, 0, "no CPU has lower priority than target");
}

// ==========================================================================
// validate_ipi_mask
// ==========================================================================

#[test]
fn validate_good_mask() {
    // Mask 0b0110, current_cpu=0, max_cpus=4 => valid.
    assert!(validate_ipi_mask(0b0110, 0, 4));
}

#[test]
fn validate_rejects_current_cpu() {
    // Bit 0 set but current_cpu is 0 => invalid.
    assert!(!validate_ipi_mask(0b0001, 0, 4));
}

#[test]
fn validate_rejects_out_of_range_bits() {
    // Bit 4 set but max_cpus is 4 => invalid (bit 4 is out of range).
    assert!(!validate_ipi_mask(1 << 4, 0, 4));
}

#[test]
fn validate_zero_mask_always_valid() {
    for cpu in 0..16u32 {
        assert!(validate_ipi_mask(0, cpu, 16), "zero mask is always valid");
    }
}

// ==========================================================================
// Negative target priorities (Zephyr cooperative range)
// ==========================================================================

#[test]
fn negative_target_prio() {
    // Target prio -5 (cooperative, very high importance).
    // CPU 1 has prio 0 > -5 => should get IPI.
    let mask = compute_ipi_mask(
        0,
        -5,
        0xFFFF_FFFF,
        &[0, 0],
        &[true, true],
        2,
        2,
    );
    assert_ne!(mask & (1 << 1), 0, "prio 0 > -5, CPU 1 should get IPI");
}

#[test]
fn both_negative_prios() {
    // Target prio -5, CPU 1 has prio -3 (> -5) => should get IPI.
    // CPU 2 has prio -10 (< -5) => should NOT get IPI.
    let mask = compute_ipi_mask(
        0,
        -5,
        0xFFFF_FFFF,
        &[0, -3, -10],
        &[true, true, true],
        3,
        3,
    );
    assert_ne!(mask & (1 << 1), 0, "prio -3 > -5, CPU 1 gets IPI");
    assert_eq!(mask & (1 << 2), 0, "prio -10 < -5, CPU 2 does not");
}

// ==========================================================================
// Mixed scenarios
// ==========================================================================

#[test]
fn mixed_active_mask_prio() {
    // 4 CPUs, current = 2.
    // CPU 0: active, in mask, prio 20 > 10 => eligible
    // CPU 1: active, NOT in mask => excluded
    // CPU 2: current => excluded
    // CPU 3: inactive => excluded
    let mask = compute_ipi_mask(
        2,          // current_cpu
        10,         // target_prio
        0b0101,     // target_cpu_mask: CPUs 0, 2
        &[20, 20, 0, 20],
        &[true, true, true, false],
        4,
        4,
    );
    assert_eq!(mask, 0b0001, "only CPU 0 qualifies");
}
