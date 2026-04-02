//! Differential equivalence tests — Spinlock Validate (FFI vs Model).
//!
//! Verifies that the FFI spinlock validation functions produce the same
//! results as the Verus-verified model functions in gale::spinlock_validate.

#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use gale::spinlock_validate::*;

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Replica of gale_spin_lock_valid.
/// Returns 1 if valid, 0 if would deadlock (same CPU holds lock).
fn ffi_spin_lock_valid(thread_cpu: usize, current_cpu_id: u32) -> i32 {
    if spin_lock_valid(thread_cpu, current_cpu_id) {
        1
    } else {
        0
    }
}

/// Replica of gale_spin_unlock_valid.
/// Returns 1 if valid (owner matches), 0 otherwise.
fn ffi_spin_unlock_valid(
    thread_cpu: usize,
    current_cpu_id: u32,
    current_thread: usize,
) -> i32 {
    if spin_unlock_valid(thread_cpu, current_cpu_id, current_thread) {
        1
    } else {
        0
    }
}

/// Replica of gale_spin_lock_compute_owner.
fn ffi_spin_lock_compute_owner(current_cpu_id: u32, current_thread: usize) -> usize {
    spin_lock_compute_owner(current_cpu_id, current_thread)
}

// =====================================================================
// Differential tests: spin_lock_valid
// =====================================================================

#[test]
fn spinlock_lock_valid_ffi_matches_model_exhaustive() {
    // thread_cpu=0 means lock is free
    for current_cpu in 0u32..MAX_CPUS {
        // Lock is free
        let ffi_result = ffi_spin_lock_valid(0, current_cpu);
        let model_result = spin_lock_valid(0, current_cpu);
        assert_eq!(ffi_result, if model_result { 1 } else { 0 },
            "free lock: cpu={current_cpu}");
        assert_eq!(ffi_result, 1, "free lock should always be valid");
    }

    // Lock is held: try all combinations of holder_cpu and current_cpu
    for holder_cpu in 0u32..MAX_CPUS {
        for thread_ptr_base in [0usize, THREAD_ALIGN, THREAD_ALIGN * 2, THREAD_ALIGN * 100] {
            let thread_cpu = thread_ptr_base | (holder_cpu as usize);
            // thread_cpu == 0 means free, skip if it happens
            if thread_cpu == 0 {
                continue;
            }

            for current_cpu in 0u32..MAX_CPUS {
                let ffi_result = ffi_spin_lock_valid(thread_cpu, current_cpu);
                let model_result = spin_lock_valid(thread_cpu, current_cpu);

                assert_eq!(ffi_result, if model_result { 1 } else { 0 },
                    "held lock: thread_cpu=0x{thread_cpu:X}, holder_cpu={holder_cpu}, current_cpu={current_cpu}");

                // SV2: should return false (0) iff same CPU
                if holder_cpu == current_cpu {
                    assert_eq!(ffi_result, 0,
                        "SV2: same CPU should be invalid: cpu={current_cpu}");
                } else {
                    assert_eq!(ffi_result, 1,
                        "SV2: different CPU should be valid: holder={holder_cpu}, current={current_cpu}");
                }
            }
        }
    }
}

// =====================================================================
// Differential tests: spin_unlock_valid
// =====================================================================

#[test]
fn spinlock_unlock_valid_ffi_matches_model_exhaustive() {
    for cpu_id in 0u32..MAX_CPUS {
        for thread_ptr in [0usize, THREAD_ALIGN, THREAD_ALIGN * 2, THREAD_ALIGN * 42] {
            let owner = spin_lock_compute_owner(cpu_id, thread_ptr);

            // Correct owner: should be valid
            let ffi_result = ffi_spin_unlock_valid(owner, cpu_id, thread_ptr);
            let model_result = spin_unlock_valid(owner, cpu_id, thread_ptr);
            assert_eq!(ffi_result, if model_result { 1 } else { 0 });
            assert_eq!(ffi_result, 1,
                "SV3: correct owner should be valid: cpu={cpu_id}, thread=0x{thread_ptr:X}");

            // Wrong CPU: should be invalid
            let wrong_cpu = (cpu_id + 1) % MAX_CPUS;
            let ffi_wrong_cpu = ffi_spin_unlock_valid(owner, wrong_cpu, thread_ptr);
            let model_wrong_cpu = spin_unlock_valid(owner, wrong_cpu, thread_ptr);
            assert_eq!(ffi_wrong_cpu, if model_wrong_cpu { 1 } else { 0 });
            // (May still match if thread_ptr happens to compensate, but typically won't)

            // Wrong thread: should be invalid
            let wrong_thread = thread_ptr.wrapping_add(THREAD_ALIGN);
            let ffi_wrong_thread = ffi_spin_unlock_valid(owner, cpu_id, wrong_thread);
            let model_wrong_thread = spin_unlock_valid(owner, cpu_id, wrong_thread);
            assert_eq!(ffi_wrong_thread, if model_wrong_thread { 1 } else { 0 });
            if thread_ptr != wrong_thread {
                assert_eq!(ffi_wrong_thread, 0,
                    "SV3: wrong thread should be invalid: cpu={cpu_id}");
            }
        }
    }
}

// =====================================================================
// Differential tests: spin_lock_compute_owner
// =====================================================================

#[test]
fn spinlock_compute_owner_ffi_matches_model_exhaustive() {
    for cpu_id in 0u32..MAX_CPUS {
        for thread_ptr in [0usize, THREAD_ALIGN, THREAD_ALIGN * 2, THREAD_ALIGN * 100, THREAD_ALIGN * 0xFFFF] {
            let ffi_owner = ffi_spin_lock_compute_owner(cpu_id, thread_ptr);
            let model_owner = spin_lock_compute_owner(cpu_id, thread_ptr);

            assert_eq!(ffi_owner, model_owner,
                "compute_owner: cpu={cpu_id}, thread=0x{thread_ptr:X}");

            // SV4: CPU ID is recoverable
            assert_eq!(ffi_owner & CPU_MASK, cpu_id as usize,
                "SV4: CPU ID not recoverable: cpu={cpu_id}, thread=0x{thread_ptr:X}");

            // SV5: thread pointer is recoverable (clear low bits)
            assert_eq!(ffi_owner & !CPU_MASK, thread_ptr & !CPU_MASK,
                "SV5: thread ptr not recoverable: cpu={cpu_id}, thread=0x{thread_ptr:X}");
        }
    }
}

// =====================================================================
// Property: SV1 — owner encoding is injective
// =====================================================================

#[test]
fn spinlock_owner_encoding_injective() {
    // For distinct (cpu, thread) pairs, the encoded owner must be distinct
    let mut owners = Vec::new();
    for cpu_id in 0u32..MAX_CPUS {
        for thread_idx in 0u32..16 {
            let thread_ptr = (thread_idx as usize) * THREAD_ALIGN;
            let owner = spin_lock_compute_owner(cpu_id, thread_ptr);
            owners.push((cpu_id, thread_ptr, owner));
        }
    }

    for i in 0..owners.len() {
        for j in (i + 1)..owners.len() {
            let (cpu_a, thr_a, own_a) = owners[i];
            let (cpu_b, thr_b, own_b) = owners[j];
            if cpu_a != cpu_b || thr_a != thr_b {
                assert_ne!(own_a, own_b,
                    "SV1: encoding not injective: ({cpu_a}, 0x{thr_a:X}) and ({cpu_b}, 0x{thr_b:X}) both map to 0x{own_a:X}");
            }
        }
    }
}

// =====================================================================
// Round-trip: lock -> compute_owner -> unlock_valid
// =====================================================================

#[test]
fn spinlock_lock_unlock_roundtrip() {
    for cpu_id in 0u32..MAX_CPUS {
        for thread_ptr in [THREAD_ALIGN, THREAD_ALIGN * 2, THREAD_ALIGN * 42] {
            // Step 1: Lock is free (thread_cpu=0), lock_valid returns true
            assert_eq!(ffi_spin_lock_valid(0, cpu_id), 1,
                "free lock should be valid");

            // Step 2: Compute owner
            let owner = ffi_spin_lock_compute_owner(cpu_id, thread_ptr);

            // Step 3: Lock is now held, same CPU cannot re-acquire
            assert_eq!(ffi_spin_lock_valid(owner, cpu_id), 0,
                "same CPU cannot re-acquire");

            // Step 4: Unlock with correct owner succeeds
            assert_eq!(ffi_spin_unlock_valid(owner, cpu_id, thread_ptr), 1,
                "correct owner can unlock");

            // Step 5: Unlock with wrong CPU fails
            let other_cpu = (cpu_id + 1) % MAX_CPUS;
            assert_eq!(ffi_spin_unlock_valid(owner, other_cpu, thread_ptr), 0,
                "wrong CPU cannot unlock");
        }
    }
}
