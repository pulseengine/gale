//! Verified CPU affinity mask model for Zephyr RTOS.
//!
//! This is a formally verified port of zephyr/kernel/cpu_mask.c.
//! All safety-critical properties are proven with Verus (SMT/Z3).
//!
//! Source mapping:
//!   cpu_mask_mod              -> cpu_mask_mod           (cpu_mask.c:19-45)
//!   k_thread_cpu_mask_clear   -> (enable=0, disable=0xFFFFFFFF)
//!   k_thread_cpu_mask_enable_all -> (enable=0xFFFFFFFF, disable=0)
//!   k_thread_cpu_mask_enable  -> (enable=BIT(cpu), disable=0)
//!   k_thread_cpu_mask_disable -> (enable=0, disable=BIT(cpu))
//!   k_thread_cpu_pin          -> (enable=BIT(cpu), disable=!BIT(cpu))
//!   validate_pin_mask         -> power-of-2 check (cpu_mask.c:38-41)
//!   cpu_pin_compute           -> BIT(cpu) with bounds check
//!
//! Omitted (not safety-relevant):
//!   - K_SPINLOCK(&_sched_spinlock) — locking handled in C
//!   - z_is_thread_prevented_from_running — caller supplies `is_running`
//!   - CONFIG_POLL, CONFIG_OBJ_CORE — debug/tracing
//!   - CONFIG_USERSPACE (z_vrfy_*) — syscall marshaling
//!
//! ASIL-D verified properties:
//!   CM1: Running threads cannot have mask modified (returns EINVAL)
//!   CM2: PIN_ONLY mode requires exactly one bit set: (m & (m-1)) == 0
//!   CM3: New mask = (current | enable) & !disable
//!   CM4: Result mask is never zero (at least one CPU)
//!   CM5: Mask arithmetic is overflow-safe (bitwise ops on u32)
//!   CM6: cpu_pin_compute bounds-checks cpu_id < max_cpus <= 32

use vstd::prelude::*;
use crate::error::*;

verus! {

/// Maximum supported CPUs (matches Zephyr BUILD_ASSERT: max 16).
pub const MAX_CPUS: u32 = 16;

/// Result of a cpu_mask_mod operation.
///
/// On success, holds the new mask. On failure, holds the error code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuMaskResult {
    /// The resulting CPU affinity mask (valid only when `error == OK`).
    pub mask: u32,
    /// Error code: OK on success, EINVAL on failure.
    pub error: i32,
}

// ------------------------------------------------------------------
// Specification helpers
// ------------------------------------------------------------------

/// Spec-level power-of-2 check: exactly one bit set.
pub open spec fn is_power_of_two(m: u32) -> bool {
    m == 1u32 || m == 2u32 || m == 4u32 || m == 8u32
    || m == 16u32 || m == 32u32 || m == 64u32 || m == 128u32
    || m == 256u32 || m == 512u32 || m == 1024u32 || m == 2048u32
    || m == 4096u32 || m == 8192u32 || m == 16384u32 || m == 32768u32
    || m == 65536u32 || m == 131072u32 || m == 262144u32 || m == 524288u32
    || m == 1048576u32 || m == 2097152u32 || m == 4194304u32 || m == 8388608u32
    || m == 16777216u32 || m == 33554432u32 || m == 67108864u32 || m == 134217728u32
    || m == 268435456u32 || m == 536870912u32 || m == 1073741824u32 || m == 2147483648u32
}

/// Spec-level mask computation.
pub open spec fn compute_mask(current: u32, enable: u32, disable: u32) -> u32 {
    (current | enable) & !disable
}

// ------------------------------------------------------------------
// Operations
// ------------------------------------------------------------------

/// Check whether a mask is a valid PIN_ONLY mask (exactly one bit set).
///
/// This is the power-of-two check from cpu_mask.c:38-41:
///   `(m == 0) || ((m & (m - 1)) == 0)`
/// We strengthen to require m != 0 (a zero mask is never valid).
///
/// CM2: PIN_ONLY mode requires exactly one bit set.
pub fn validate_pin_mask(mask: u32) -> (result: bool)
    ensures
        result == (mask != 0 && (mask & sub(mask, 1u32)) == 0u32),
{
    mask != 0 && (mask & (mask - 1)) == 0
}

/// Core CPU mask modification function.
///
/// Models cpu_mask_mod (cpu_mask.c:19-45).
///
/// Parameters:
/// - `current_mask`: the thread's current cpu_mask
/// - `enable`: bits to OR into the mask
/// - `disable`: bits to AND-complement out of the mask
/// - `is_running`: true if the thread is currently running (not prevented)
/// - `pin_only`: true if CONFIG_SCHED_CPU_MASK_PIN_ONLY is enabled
///
/// CM1: Running threads cannot have mask modified (returns EINVAL).
/// CM2: PIN_ONLY mode requires exactly one bit set in the result.
/// CM3: New mask = (current | enable) & !disable.
/// CM4: Result mask is never zero.
/// CM5: All arithmetic is overflow-safe (bitwise ops on u32).
pub fn cpu_mask_mod(
    current_mask: u32,
    enable: u32,
    disable: u32,
    is_running: bool,
    pin_only: bool,
) -> (result: CpuMaskResult)
    ensures
        // CM1: running threads get EINVAL
        is_running ==> result.error == EINVAL,
        // CM3: on success, mask matches the formula
        result.error == OK ==> result.mask == (((current_mask | enable) & !disable) as u32),
        // CM4: on success, mask is never zero
        result.error == OK ==> result.mask != 0u32,
        // CM2: on success with pin_only, exactly one bit is set
        (result.error == OK && pin_only) ==>
            (result.mask != 0u32 && (result.mask & sub(result.mask, 1u32)) == 0u32),
        // Error codes are constrained
        result.error == OK || result.error == EINVAL,
{
    // CM1: refuse to modify a running thread's mask
    if is_running {
        return CpuMaskResult { mask: current_mask, error: EINVAL };
    }

    // CM3: compute the new mask
    let new_mask: u32 = (current_mask | enable) & !disable;

    // CM4: at least one CPU must remain enabled
    if new_mask == 0 {
        return CpuMaskResult { mask: current_mask, error: EINVAL };
    }

    // CM2: PIN_ONLY requires exactly one bit set
    if pin_only && (new_mask & (new_mask - 1)) != 0 {
        return CpuMaskResult { mask: current_mask, error: EINVAL };
    }

    CpuMaskResult { mask: new_mask, error: OK }
}

/// Compute the pin mask for a specific CPU.
///
/// Models the BIT(cpu) computation from k_thread_cpu_pin (cpu_mask.c:69).
/// Returns `Ok(1u32 << cpu_id)` if `cpu_id < max_cpus` and `max_cpus <= 32`,
/// otherwise returns `Err(EINVAL)`.
///
/// CM6: bounds check ensures shift is within u32 range.
pub fn cpu_pin_compute(cpu_id: u32, max_cpus: u32) -> (result: Result<u32, i32>)
    ensures
        // Bounds failure
        (cpu_id >= max_cpus || max_cpus > 32) ==> result.is_err(),
        result.is_err() ==> result == Err::<u32, i32>(EINVAL),
        // Success: result is a single-bit mask
        result.is_ok() ==> {
            let m = result.unwrap();
            &&& cpu_id < 32
            &&& is_power_of_two(m)
        },
{
    if max_cpus > 32 || cpu_id >= max_cpus {
        return Err(EINVAL);
    }

    // cpu_id < 32 guaranteed by the bounds check above (max_cpus <= 32)
    let mask: u32 = 1u32 << cpu_id;

    // Proof hint: 1 << cpu_id is a power of two for any cpu_id < 32
    proof {
        assert(mask == 1u32 || mask == 2u32 || mask == 4u32 || mask == 8u32
            || mask == 16u32 || mask == 32u32 || mask == 64u32 || mask == 128u32
            || mask == 256u32 || mask == 512u32 || mask == 1024u32 || mask == 2048u32
            || mask == 4096u32 || mask == 8192u32 || mask == 16384u32 || mask == 32768u32
            || mask == 65536u32 || mask == 131072u32 || mask == 262144u32 || mask == 524288u32
            || mask == 1048576u32 || mask == 2097152u32 || mask == 4194304u32 || mask == 8388608u32
            || mask == 16777216u32 || mask == 33554432u32 || mask == 67108864u32 || mask == 134217728u32
            || mask == 268435456u32 || mask == 536870912u32 || mask == 1073741824u32 || mask == 2147483648u32
        ) by(bit_vector)
            requires cpu_id < 32u32, mask == 1u32 << cpu_id;
    }

    Ok(mask)
}

} // verus!
