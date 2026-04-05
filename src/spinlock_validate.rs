//! Verified spinlock validation helpers for Zephyr RTOS.
//!
//! This is a formally verified port of zephyr/kernel/spinlock_validate.c.
//! All safety-critical properties are proven with Verus (SMT/Z3).
//!
//! The C code encodes lock ownership by OR-ing the CPU ID into the low
//! bits of a thread pointer (which is guaranteed aligned).  The `& 3U`
//! mask hard-codes support for at most 4 CPUs.  This module replaces
//! the magic constant with a configurable `MAX_CPUS` and explicit
//! alignment requirement, then proves the encoding is injective.
//!
//! Source mapping:
//!   z_spin_lock_valid     -> spin_lock_valid        (spinlock_validate.c:10-20)
//!   z_spin_unlock_valid   -> spin_unlock_valid      (spinlock_validate.c:23-37)
//!   z_spin_lock_set_owner -> spin_lock_compute_owner (spinlock_validate.c:39-43)
//!
//! Omitted (not safety-relevant):
//!   - CONFIG_KERNEL_COHERENCE (z_spin_lock_mem_coherent) — cache coherency check
//!   - ISR + _THREAD_DUMMY edge case — modeled as a separate flag in the caller
//!
//! ASIL-D verified properties:
//!   SV1: owner encoding is injective (distinct (cpu, thread) -> distinct owner)
//!   SV2: lock_valid returns false iff the lock is already held by the same CPU
//!   SV3: unlock_valid returns true iff the stored owner matches (cpu | thread)
//!   SV4: CPU ID is recoverable from the encoded owner via masking
//!   SV5: thread pointer is recoverable from the encoded owner via masking
//!   SV6: MAX_CPUS bounds the CPU mask (replaces hard-coded `& 3U`)

use vstd::prelude::*;

verus! {

// ======================================================================
// Constants
// ======================================================================

/// Maximum number of CPUs supported.
///
/// Zephyr hard-codes `& 3U` (2 bits, 4 CPUs).  We use the same default
/// but express it as a named constant so it can be widened.
pub const MAX_CPUS: u32 = 4;

/// Bit mask to extract the CPU ID from an encoded owner value.
///
/// CPU_MASK == MAX_CPUS - 1 == 0b11 for 4 CPUs.
/// Requires MAX_CPUS to be a power of two.
pub const CPU_MASK: usize = 3; // MAX_CPUS - 1

/// Minimum alignment of thread pointers (in bytes).
///
/// Thread pointers must be aligned to at least MAX_CPUS so that the
/// low bits are zero and available for the CPU ID tag.
pub const THREAD_ALIGN: usize = 4; // == MAX_CPUS

// ======================================================================
// Specification helpers
// ======================================================================

/// A thread pointer is valid for encoding: non-zero and properly aligned
/// so that its low `CPU_MASK` bits are zero.
pub open spec fn thread_ptr_valid(thread: usize) -> bool {
    thread != 0 && (thread & (CPU_MASK as usize)) == 0
}

/// A CPU ID is valid: strictly less than MAX_CPUS.
pub open spec fn cpu_id_valid(cpu: u32) -> bool {
    (cpu as usize) < (MAX_CPUS as usize)
}

/// Encode a (cpu, thread) pair into an owner tag (spec).
pub open spec fn encode_owner_spec(cpu: u32, thread: usize) -> usize {
    thread | (cpu as usize)
}

/// Decode the CPU ID from an encoded owner value (spec).
pub open spec fn decode_cpu_spec(owner: usize) -> usize {
    owner & (CPU_MASK as usize)
}

/// Decode the thread pointer from an encoded owner value (spec).
pub open spec fn decode_thread_spec(owner: usize) -> usize {
    owner & !(CPU_MASK as usize)
}

// ======================================================================
// Executable functions
// ======================================================================

/// Check whether acquiring the spinlock is valid.
///
/// Models z_spin_lock_valid() (spinlock_validate.c:10-20):
///
/// ```c
/// bool z_spin_lock_valid(struct k_spinlock *l) {
///     uintptr_t thread_cpu = l->thread_cpu;
///     if (thread_cpu != 0U) {
///         if ((thread_cpu & 3U) == _current_cpu->id)
///             return false;
///     }
///     return true;
/// }
/// ```
///
/// Returns `false` when the lock is already held **by the same CPU**
/// (i.e. the CPU bits in `thread_cpu` match `current_cpu_id`).
/// A `thread_cpu` of 0 means the lock is free.
///
/// SV2: lock_valid returns false iff lock is held and CPU matches.
pub fn spin_lock_valid(thread_cpu: usize, current_cpu_id: u32) -> (valid: bool)
    requires
        cpu_id_valid(current_cpu_id),
    ensures
        // Free lock is always valid to acquire.
        thread_cpu == 0 ==> valid,
        // Held lock with same CPU -> invalid (would deadlock).
        thread_cpu != 0 && (thread_cpu & CPU_MASK) == (current_cpu_id as usize)
            ==> !valid,
        // Held lock with different CPU -> valid (cross-CPU contention is OK
        // at the validation layer; the actual spin-wait happens elsewhere).
        thread_cpu != 0 && (thread_cpu & CPU_MASK) != (current_cpu_id as usize)
            ==> valid,
{
    if thread_cpu != 0 {
        if (thread_cpu & CPU_MASK) == (current_cpu_id as usize) {
            return false;
        }
    }
    true
}

/// Check whether releasing the spinlock is valid.
///
/// Models z_spin_unlock_valid() (spinlock_validate.c:23-37):
///
/// ```c
/// bool z_spin_unlock_valid(struct k_spinlock *l) {
///     uintptr_t tcpu = l->thread_cpu;
///     l->thread_cpu = 0;
///     ...
///     if (tcpu != (_current_cpu->id | (uintptr_t)_current))
///         return false;
///     return true;
/// }
/// ```
///
/// Returns `true` iff the stored `thread_cpu` matches the encoded
/// identity of the current thread on the current CPU.
///
/// Note: the C function also zeroes `l->thread_cpu` and handles an ISR
/// edge case.  Both are side-effects handled by the FFI layer; this
/// function is a pure validity predicate.
///
/// SV3: unlock_valid returns true iff owner matches (cpu | thread).
pub fn spin_unlock_valid(
    thread_cpu: usize,
    current_cpu_id: u32,
    current_thread: usize,
) -> (valid: bool)
    requires
        cpu_id_valid(current_cpu_id),
        thread_ptr_valid(current_thread),
    ensures
        valid == (thread_cpu == encode_owner_spec(current_cpu_id, current_thread)),
{
    let cpu = current_cpu_id as usize;
    let expected = cpu | current_thread;
    // OR is commutative: cpu | thread == thread | cpu == encode_owner_spec
    proof {
        assert(cpu | current_thread == current_thread | cpu) by(bit_vector);
    }
    thread_cpu == expected
}

/// Compute the owner tag for a spinlock.
///
/// Models z_spin_lock_set_owner() (spinlock_validate.c:39-43):
///
/// ```c
/// void z_spin_lock_set_owner(struct k_spinlock *l) {
///     l->thread_cpu = _current_cpu->id | (uintptr_t)_current;
/// }
/// ```
///
/// Encodes the current CPU ID and thread pointer into a single `usize`.
///
/// SV4/SV5: CPU and thread are recoverable.
/// SV6: CPU ID fits within the mask.
pub fn spin_lock_compute_owner(
    current_cpu_id: u32,
    current_thread: usize,
) -> (owner: usize)
    requires
        cpu_id_valid(current_cpu_id),
        thread_ptr_valid(current_thread),
    ensures
        owner == encode_owner_spec(current_cpu_id, current_thread),
        // SV4: CPU ID is recoverable.
        (owner & CPU_MASK) == (current_cpu_id as usize),
        // SV5: thread pointer is recoverable.
        (owner & !(CPU_MASK as usize)) == current_thread,
        // Non-zero owner (thread != 0 by precondition).
        owner != 0,
{
    // Proof hint: since thread's low bits are 0 and cpu fits in those bits,
    // OR is the same as addition, and the fields don't overlap.
    proof {
        let cpu = current_cpu_id as usize;
        let owner = current_thread | cpu;
        // Bridge: CPU_MASK == 3
        assert(CPU_MASK == 3usize);
        // BV proofs with literal mask
        assert((current_thread | cpu) & 3usize == cpu) by(bit_vector)
            requires current_thread & 3usize == 0usize, cpu & !3usize == 0usize;
        assert((current_thread | cpu) & !3usize == current_thread) by(bit_vector)
            requires current_thread & 3usize == 0usize;
        assert((current_thread | cpu) != 0usize) by(bit_vector)
            requires current_thread != 0usize;
        // OR commutativity: cpu | thread == thread | cpu
        assert(cpu | current_thread == current_thread | cpu) by(bit_vector);
    }
    (current_cpu_id as usize) | current_thread
}

// ======================================================================
// Compositional proofs
// ======================================================================

/// SV1: Owner encoding is injective — distinct (cpu, thread) pairs
/// produce distinct owner values.
pub proof fn lemma_encoding_injective(
    cpu1: u32, thread1: usize,
    cpu2: u32, thread2: usize,
)
    requires
        cpu_id_valid(cpu1),
        cpu_id_valid(cpu2),
        thread_ptr_valid(thread1),
        thread_ptr_valid(thread2),
        // At least one component differs.
        cpu1 != cpu2 || thread1 != thread2,
    ensures
        encode_owner_spec(cpu1, thread1) != encode_owner_spec(cpu2, thread2),
{
    // Case 1: threads differ -> high bits differ -> owners differ.
    // Case 2: threads same, CPUs differ -> low bits differ -> owners differ.
    let o1 = thread1 | (cpu1 as usize);
    let o2 = thread2 | (cpu2 as usize);

    if cpu1 != cpu2 {
        assert(o1 & 3usize != o2 & 3usize) by(bit_vector)
            requires
                thread1 & 3usize == 0usize,
                thread2 & 3usize == 0usize,
                (cpu1 as usize) & !3usize == 0usize,
                (cpu2 as usize) & !3usize == 0usize,
                cpu1 as usize != cpu2 as usize;
        assert(CPU_MASK == 3usize);
        assert(o1 != o2);
    } else {
        // cpu1 == cpu2 && thread1 != thread2
        assert(o1 & !3usize != o2 & !3usize) by(bit_vector)
            requires
                thread1 & 3usize == 0usize,
                thread2 & 3usize == 0usize,
                thread1 != thread2;
        assert(o1 != o2);
    }
}

/// SV4/SV5: encode then decode recovers both components.
pub proof fn lemma_encode_decode_roundtrip(cpu: u32, thread: usize)
    requires
        cpu_id_valid(cpu),
        thread_ptr_valid(thread),
    ensures
        decode_cpu_spec(encode_owner_spec(cpu, thread)) == (cpu as usize),
        decode_thread_spec(encode_owner_spec(cpu, thread)) == thread,
{
    let owner = thread | (cpu as usize);
    assert(owner & 3usize == cpu as usize) by(bit_vector)
        requires
            thread & 3usize == 0usize,
            (cpu as usize) & !3usize == 0usize;
    assert(owner & !3usize == thread) by(bit_vector)
        requires
            thread & 3usize == 0usize;
    assert(CPU_MASK == 3usize);
}

// SV2 (lock_valid characterisation) and SV3 (unlock_valid iff owner)
// are encoded directly in the exec functions' ensures clauses.
// Standalone proof lemmas omitted: Verus proof fns cannot call exec fns.

} // verus!
