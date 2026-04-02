//! Integration tests for spinlock validation — exercises owner encoding,
//! lock validity, and unlock validity across all edge cases.
//!
//! These tests run under: cargo test, miri, sanitizers.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::wildcard_enum_match_arm,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use gale::spinlock_validate::*;

/// Helper: build an aligned thread pointer.
/// Real thread pointers are at least 4-byte aligned (THREAD_ALIGN).
fn aligned_thread(n: usize) -> usize {
    n * THREAD_ALIGN
}

// ==========================================================================
// SV1: Owner encode/decode roundtrip
// ==========================================================================

#[test]
fn encode_decode_roundtrip_all_cpus() {
    let thread = aligned_thread(42); // 168, aligned
    for cpu in 0..MAX_CPUS {
        let owner = spin_lock_compute_owner(cpu, thread);
        // Decode CPU
        assert_eq!(owner & CPU_MASK, cpu as usize, "CPU decode failed for cpu={cpu}");
        // Decode thread
        assert_eq!(owner & !CPU_MASK, thread, "thread decode failed for cpu={cpu}");
    }
}

#[test]
fn encode_decode_roundtrip_various_threads() {
    let threads: &[usize] = &[
        aligned_thread(1),
        aligned_thread(100),
        aligned_thread(0x1000),
        aligned_thread(0xFFFF),
        aligned_thread(0x1_0000),
    ];
    for &thread in threads {
        for cpu in 0..MAX_CPUS {
            let owner = spin_lock_compute_owner(cpu, thread);
            assert_eq!(owner & CPU_MASK, cpu as usize);
            assert_eq!(owner & !CPU_MASK, thread);
        }
    }
}

#[test]
fn encoding_is_injective() {
    // Distinct (cpu, thread) pairs must produce distinct owners.
    let threads: &[usize] = &[
        aligned_thread(1),
        aligned_thread(2),
        aligned_thread(3),
        aligned_thread(100),
    ];
    let mut owners = Vec::new();
    for &thread in threads {
        for cpu in 0..MAX_CPUS {
            owners.push(spin_lock_compute_owner(cpu, thread));
        }
    }
    // Check all pairs are distinct.
    for i in 0..owners.len() {
        for j in (i + 1)..owners.len() {
            assert_ne!(owners[i], owners[j], "collision at indices {i} and {j}");
        }
    }
}

// ==========================================================================
// SV2: Valid lock detection for all valid CPU IDs
// ==========================================================================

#[test]
fn free_lock_is_valid_for_all_cpus() {
    for cpu in 0..MAX_CPUS {
        assert!(
            spin_lock_valid(0, cpu),
            "free lock should be valid for cpu={cpu}"
        );
    }
}

#[test]
fn lock_held_by_different_cpu_is_valid() {
    let thread = aligned_thread(10);
    for owner_cpu in 0..MAX_CPUS {
        let owner = spin_lock_compute_owner(owner_cpu, thread);
        for current_cpu in 0..MAX_CPUS {
            if current_cpu != owner_cpu {
                assert!(
                    spin_lock_valid(owner, current_cpu),
                    "cross-CPU should be valid: owner_cpu={owner_cpu}, current_cpu={current_cpu}"
                );
            }
        }
    }
}

// ==========================================================================
// SV2 (negative): Invalid lock detection — same CPU
// ==========================================================================

#[test]
fn lock_held_by_same_cpu_is_invalid() {
    let thread = aligned_thread(10);
    for cpu in 0..MAX_CPUS {
        let owner = spin_lock_compute_owner(cpu, thread);
        assert!(
            !spin_lock_valid(owner, cpu),
            "same-CPU should be invalid: cpu={cpu}"
        );
    }
}

// ==========================================================================
// SV3: Unlock validation — correct owner
// ==========================================================================

#[test]
fn unlock_valid_with_correct_owner() {
    let thread = aligned_thread(50);
    for cpu in 0..MAX_CPUS {
        let owner = spin_lock_compute_owner(cpu, thread);
        assert!(
            spin_unlock_valid(owner, cpu, thread),
            "unlock should be valid for correct owner: cpu={cpu}"
        );
    }
}

// ==========================================================================
// SV3 (negative): Unlock validation — wrong CPU or wrong thread
// ==========================================================================

#[test]
fn unlock_invalid_with_wrong_cpu() {
    let thread = aligned_thread(50);
    for owner_cpu in 0..MAX_CPUS {
        let owner = spin_lock_compute_owner(owner_cpu, thread);
        for wrong_cpu in 0..MAX_CPUS {
            if wrong_cpu != owner_cpu {
                assert!(
                    !spin_unlock_valid(owner, wrong_cpu, thread),
                    "unlock should fail with wrong cpu: owner={owner_cpu}, wrong={wrong_cpu}"
                );
            }
        }
    }
}

#[test]
fn unlock_invalid_with_wrong_thread() {
    let thread_a = aligned_thread(50);
    let thread_b = aligned_thread(51);
    for cpu in 0..MAX_CPUS {
        let owner = spin_lock_compute_owner(cpu, thread_a);
        assert!(
            !spin_unlock_valid(owner, cpu, thread_b),
            "unlock should fail with wrong thread: cpu={cpu}"
        );
    }
}

#[test]
fn unlock_invalid_with_zero_thread_cpu() {
    // If thread_cpu is 0 (free lock), unlock should fail for any (cpu, thread).
    let thread = aligned_thread(10);
    for cpu in 0..MAX_CPUS {
        assert!(
            !spin_unlock_valid(0, cpu, thread),
            "unlock of free lock should fail: cpu={cpu}"
        );
    }
}

// ==========================================================================
// Edge cases
// ==========================================================================

#[test]
fn cpu_id_zero() {
    let thread = aligned_thread(7);
    let owner = spin_lock_compute_owner(0, thread);
    assert_eq!(owner & CPU_MASK, 0);
    assert_eq!(owner & !CPU_MASK, thread);
    // Same-CPU check still works.
    assert!(!spin_lock_valid(owner, 0));
    assert!(spin_unlock_valid(owner, 0, thread));
}

#[test]
fn max_cpu_id() {
    let last_cpu = MAX_CPUS - 1;
    let thread = aligned_thread(7);
    let owner = spin_lock_compute_owner(last_cpu, thread);
    assert_eq!(owner & CPU_MASK, last_cpu as usize);
    assert_eq!(owner & !CPU_MASK, thread);
    assert!(!spin_lock_valid(owner, last_cpu));
    assert!(spin_unlock_valid(owner, last_cpu, thread));
}

#[test]
fn minimal_aligned_thread() {
    // Smallest valid thread pointer: THREAD_ALIGN (one alignment unit).
    let thread = THREAD_ALIGN; // == 4
    for cpu in 0..MAX_CPUS {
        let owner = spin_lock_compute_owner(cpu, thread);
        assert_ne!(owner, 0);
        assert_eq!(owner & CPU_MASK, cpu as usize);
        assert_eq!(owner & !CPU_MASK, thread);
    }
}

#[test]
fn large_thread_pointer() {
    // Simulate a high-address thread pointer (e.g. kernel space).
    let thread = aligned_thread(0x2000_0000);
    for cpu in 0..MAX_CPUS {
        let owner = spin_lock_compute_owner(cpu, thread);
        assert_eq!(owner & CPU_MASK, cpu as usize);
        assert_eq!(owner & !CPU_MASK, thread);
        assert!(spin_unlock_valid(owner, cpu, thread));
    }
}

#[test]
fn owner_is_nonzero_for_all_valid_inputs() {
    // For any valid (cpu, thread), the owner must be non-zero,
    // because thread != 0.
    for n in 1..=100 {
        let thread = aligned_thread(n);
        for cpu in 0..MAX_CPUS {
            let owner = spin_lock_compute_owner(cpu, thread);
            assert_ne!(owner, 0, "owner must be non-zero: n={n}, cpu={cpu}");
        }
    }
}
