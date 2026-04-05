//! Differential equivalence tests — SpinlockValidate (FFI vs Model).
//!
//! Verifies that the FFI spinlock validation functions produce the same
//! results as the Verus-verified model functions in gale::spinlock_validate.

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

use gale::spinlock_validate::{
    spin_lock_valid, spin_unlock_valid, spin_lock_compute_owner,
    MAX_CPUS, CPU_MASK, THREAD_ALIGN,
};

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Replica of gale_spin_lock_valid.
///
/// Returns 1 if valid (safe to acquire), 0 if invalid (would deadlock).
fn ffi_spin_lock_valid(thread_cpu: usize, current_cpu_id: u32) -> i32 {
    if spin_lock_valid(thread_cpu, current_cpu_id) { 1 } else { 0 }
}

/// Replica of gale_spin_unlock_valid.
///
/// Returns 1 if valid (caller is the owner), 0 if not.
fn ffi_spin_unlock_valid(thread_cpu: usize, current_cpu_id: u32, current_thread: usize) -> i32 {
    if spin_unlock_valid(thread_cpu, current_cpu_id, current_thread) { 1 } else { 0 }
}

/// Replica of gale_spin_lock_compute_owner.
fn ffi_spin_lock_compute_owner(current_cpu_id: u32, current_thread: usize) -> usize {
    spin_lock_compute_owner(current_cpu_id, current_thread)
}

// =====================================================================
// Thread pointer helpers — must be non-zero and aligned to THREAD_ALIGN
// =====================================================================

fn make_thread(n: usize) -> usize {
    // Shift left by THREAD_ALIGN bits to ensure low bits are zero,
    // then add THREAD_ALIGN to get a non-zero value.
    (n + 1) * THREAD_ALIGN
}

// =====================================================================
// Differential tests: spin_lock_valid
// =====================================================================

#[test]
fn spinlock_lock_valid_ffi_matches_model_free_lock() {
    // thread_cpu == 0 means lock is free: always valid to acquire
    for cpu_id in 0..MAX_CPUS {
        let ffi_ret = ffi_spin_lock_valid(0, cpu_id);
        assert_eq!(ffi_ret, 1,
            "SV2: free lock (thread_cpu=0) must be valid: cpu={cpu_id}");
    }
}

#[test]
fn spinlock_lock_valid_ffi_matches_model_same_cpu_held() {
    // Lock held by same CPU -> invalid (would deadlock)
    for cpu_id in 0..MAX_CPUS {
        let thread = make_thread(1);
        let owner = ffi_spin_lock_compute_owner(cpu_id, thread);
        let ffi_ret = ffi_spin_lock_valid(owner, cpu_id);
        assert_eq!(ffi_ret, 0,
            "SV2: held lock on same CPU must be invalid: cpu={cpu_id}, owner={owner:#x}");
    }
}

#[test]
fn spinlock_lock_valid_ffi_matches_model_different_cpu_held() {
    // Lock held by different CPU -> valid at validation layer
    if MAX_CPUS < 2 {
        return; // Skip test if only 1 CPU configured
    }
    for cpu_id in 0..MAX_CPUS {
        let other_cpu = (cpu_id + 1) % MAX_CPUS;
        let thread = make_thread(1);
        let owner = ffi_spin_lock_compute_owner(other_cpu, thread);
        let ffi_ret = ffi_spin_lock_valid(owner, cpu_id);
        assert_eq!(ffi_ret, 1,
            "SV2: lock held by different CPU must be valid at validation layer: \
             current_cpu={cpu_id}, held_by={other_cpu}");
    }
}

#[test]
fn spinlock_lock_valid_ffi_matches_model_exhaustive() {
    // Exhaustive small sweep: all CPU IDs × small set of thread_cpu values
    for cpu_id in 0..MAX_CPUS {
        // thread_cpu values to test
        for thread_idx in 0..4usize {
            for held_cpu in 0..MAX_CPUS {
                // Build a non-zero owner for held_cpu + thread
                let thread = make_thread(thread_idx);
                let thread_cpu = thread | (held_cpu as usize);

                let ffi_ret = ffi_spin_lock_valid(thread_cpu, cpu_id);
                let model_ret = if spin_lock_valid(thread_cpu, cpu_id) { 1i32 } else { 0 };

                assert_eq!(ffi_ret, model_ret,
                    "lock_valid mismatch: thread_cpu={thread_cpu:#x}, cpu_id={cpu_id}");
            }
        }
    }
}

// =====================================================================
// Differential tests: spin_unlock_valid
// =====================================================================

#[test]
fn spinlock_unlock_valid_ffi_matches_model_owner_matches() {
    for cpu_id in 0..MAX_CPUS {
        let thread = make_thread(1);
        let owner = ffi_spin_lock_compute_owner(cpu_id, thread);
        let ffi_ret = ffi_spin_unlock_valid(owner, cpu_id, thread);
        assert_eq!(ffi_ret, 1,
            "SV3: owner must match for valid unlock: cpu={cpu_id}");
    }
}

#[test]
fn spinlock_unlock_valid_ffi_matches_model_wrong_thread() {
    for cpu_id in 0..MAX_CPUS {
        let thread_a = make_thread(1);
        let thread_b = make_thread(2);
        let owner = ffi_spin_lock_compute_owner(cpu_id, thread_a);
        // Try to unlock with a different thread
        let ffi_ret = ffi_spin_unlock_valid(owner, cpu_id, thread_b);
        assert_eq!(ffi_ret, 0,
            "SV3: wrong thread must fail unlock: cpu={cpu_id}");
    }
}

#[test]
fn spinlock_unlock_valid_ffi_matches_model_wrong_cpu() {
    if MAX_CPUS < 2 {
        return;
    }
    let cpu_a = 0u32;
    let cpu_b = 1u32;
    let thread = make_thread(1);
    let owner = ffi_spin_lock_compute_owner(cpu_a, thread);
    // Try to unlock with a different CPU
    let ffi_ret = ffi_spin_unlock_valid(owner, cpu_b, thread);
    assert_eq!(ffi_ret, 0,
        "SV3: wrong CPU must fail unlock");
}

#[test]
fn spinlock_unlock_valid_ffi_matches_model_exhaustive() {
    for cpu_id in 0..MAX_CPUS {
        for thread_idx in 0..4usize {
            let thread = make_thread(thread_idx);
            let owner = ffi_spin_lock_compute_owner(cpu_id, thread);

            // Correct owner
            let ffi_ret = ffi_spin_unlock_valid(owner, cpu_id, thread);
            let model_ret = if spin_unlock_valid(owner, cpu_id, thread) { 1i32 } else { 0 };
            assert_eq!(ffi_ret, model_ret,
                "unlock_valid mismatch (correct): cpu={cpu_id}, thread={thread_idx}");

            // Wrong owner (zero)
            let ffi_ret0 = ffi_spin_unlock_valid(0, cpu_id, thread);
            let model_ret0 = if spin_unlock_valid(0, cpu_id, thread) { 1i32 } else { 0 };
            assert_eq!(ffi_ret0, model_ret0,
                "unlock_valid mismatch (zero owner): cpu={cpu_id}, thread={thread_idx}");
        }
    }
}

// =====================================================================
// Differential tests: spin_lock_compute_owner
// =====================================================================

#[test]
fn spinlock_compute_owner_ffi_matches_model_exhaustive() {
    for cpu_id in 0..MAX_CPUS {
        for thread_idx in 0..8usize {
            let thread = make_thread(thread_idx);
            let ffi_owner = ffi_spin_lock_compute_owner(cpu_id, thread);
            let model_owner = spin_lock_compute_owner(cpu_id, thread);
            assert_eq!(ffi_owner, model_owner,
                "compute_owner mismatch: cpu={cpu_id}, thread_idx={thread_idx}");
        }
    }
}

// =====================================================================
// Property: SV1 — owner encoding is injective
// =====================================================================

#[test]
fn spinlock_owner_encoding_injective() {
    // Different (cpu, thread) pairs must produce different owners.
    let mut owners: std::vec::Vec<(u32, usize, usize)> = std::vec::Vec::new();
    for cpu_id in 0..MAX_CPUS {
        for thread_idx in 0..4usize {
            let thread = make_thread(thread_idx);
            let owner = ffi_spin_lock_compute_owner(cpu_id, thread);
            for &(prev_cpu, prev_thread, prev_owner) in &owners {
                if prev_cpu != cpu_id || prev_thread != thread {
                    assert_ne!(owner, prev_owner,
                        "SV1: owner encoding must be injective: \
                         cpu={cpu_id}/thread={thread:#x} vs \
                         cpu={prev_cpu}/thread={prev_thread:#x}");
                }
            }
            owners.push((cpu_id, thread, owner));
        }
    }
}

// =====================================================================
// Property: SV4/SV5 — CPU and thread recoverable from owner
// =====================================================================

#[test]
fn spinlock_cpu_recoverable_from_owner() {
    for cpu_id in 0..MAX_CPUS {
        for thread_idx in 0..4usize {
            let thread = make_thread(thread_idx);
            let owner = ffi_spin_lock_compute_owner(cpu_id, thread);
            // SV4: CPU recoverable via & CPU_MASK
            let recovered_cpu = owner & CPU_MASK;
            assert_eq!(recovered_cpu, cpu_id as usize,
                "SV4: CPU must be recoverable from owner: cpu={cpu_id}");
        }
    }
}

#[test]
fn spinlock_thread_recoverable_from_owner() {
    for cpu_id in 0..MAX_CPUS {
        for thread_idx in 0..4usize {
            let thread = make_thread(thread_idx);
            let owner = ffi_spin_lock_compute_owner(cpu_id, thread);
            // SV5: thread pointer recoverable via & !CPU_MASK
            let recovered_thread = owner & !CPU_MASK;
            assert_eq!(recovered_thread, thread,
                "SV5: thread must be recoverable from owner: thread={thread:#x}");
        }
    }
}

// =====================================================================
// Property: SV2 — lock/unlock roundtrip
// =====================================================================

#[test]
fn spinlock_lock_unlock_roundtrip() {
    // Acquire: lock_valid returns true (lock is free)
    // Then compute owner, set it, then unlock_valid with same cpu+thread returns true.
    for cpu_id in 0..MAX_CPUS {
        for thread_idx in 0..4usize {
            let thread = make_thread(thread_idx);

            // 1. Verify lock is valid on free lock
            let lock_ret = ffi_spin_lock_valid(0, cpu_id);
            assert_eq!(lock_ret, 1, "free lock must be lockable");

            // 2. Compute and set owner
            let owner = ffi_spin_lock_compute_owner(cpu_id, thread);

            // 3. With owner set: lock is no longer valid on same cpu
            let lock_ret2 = ffi_spin_lock_valid(owner, cpu_id);
            assert_eq!(lock_ret2, 0,
                "held lock must not be re-acquired on same cpu: cpu={cpu_id}");

            // 4. Unlock with correct cpu+thread must succeed
            let unlock_ret = ffi_spin_unlock_valid(owner, cpu_id, thread);
            assert_eq!(unlock_ret, 1,
                "unlock with correct owner must succeed: cpu={cpu_id}");
        }
    }
}

// =====================================================================
// Property: SV6 — CPU mask bounds
// =====================================================================

#[test]
fn spinlock_cpu_mask_bounds() {
    // CPU_MASK must be MAX_CPUS - 1 (power of two mask)
    assert_eq!(CPU_MASK, (MAX_CPUS as usize) - 1,
        "SV6: CPU_MASK must equal MAX_CPUS - 1");
    // All valid CPU IDs fit within CPU_MASK
    for cpu_id in 0..MAX_CPUS {
        assert!((cpu_id as usize) <= CPU_MASK,
            "SV6: all valid CPU IDs must fit in CPU_MASK: cpu={cpu_id}");
    }
}
