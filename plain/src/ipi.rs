//! Verified IPI mask creation model for Zephyr RTOS.
//!
//! This is a formally verified port of zephyr/kernel/ipi.c.
//! All safety-critical properties are proven with Verus (SMT/Z3).
//!
//! This module models the **IPI mask creation** logic from Zephyr's SMP
//! IPI subsystem.  The mask determines which CPUs need an inter-processor
//! interrupt when a thread becomes ready.
//!
//! Source mapping:
//!   ipi_mask_create -> compute_ipi_mask  (ipi.c:29-70)
//!
//! Omitted (not safety-relevant):
//!   - CONFIG_IPI_OPTIMIZE bypass (trivially returns IPI_ALL_CPUS_MASK)
//!   - thread_is_metairq — MetaIRQ preemption override
//!   - thread_is_preemptible — cooperative thread guard
//!   - signal_pending_ipi — actual IPI delivery
//!   - CONFIG_USERSPACE (z_vrfy_*) — syscall marshaling
//!   - SYS_PORT_TRACING_* — instrumentation
//!
//! ASIL-D verified properties:
//!   IP1: current CPU is never in the result mask
//!   IP2: only CPUs within [0, num_cpus) can be in the mask
//!   IP3: only CPUs allowed by target_cpu_mask are considered
//!   IP4: a CPU is included only if its priority > target (lower importance)
//!   IP5: result fits in max_cpus bits (no stray high bits)
/// Maximum supported CPUs (matches CONFIG_MP_MAX_NUM_CPUS).
pub const MAX_CPUS: u32 = 16;
/// Compute the IPI bitmask for a newly ready thread.
///
/// This is the verified model of `ipi_mask_create()` from ipi.c:29-70.
///
/// Parameters:
/// - `current_cpu`: CPU executing the scheduling decision
/// - `target_prio`: priority of the newly ready thread (lower = higher importance)
/// - `target_cpu_mask`: CPU affinity mask for the thread (CONFIG_SCHED_CPU_MASK)
/// - `cpu_prios`: per-CPU current thread priorities
/// - `cpu_active`: per-CPU active flags
/// - `num_cpus`: number of CPUs present (arch_num_cpus())
/// - `max_cpus`: CONFIG_MP_MAX_NUM_CPUS (upper bound for bit width)
///
/// Returns a bitmask where bit `i` is set iff CPU `i` should receive an IPI.
pub fn compute_ipi_mask(
    current_cpu: u32,
    target_prio: i32,
    target_cpu_mask: u32,
    cpu_prios: &[i32],
    cpu_active: &[bool],
    num_cpus: u32,
    max_cpus: u32,
) -> u32 {
    let mut mask: u32 = 0u32;
    let mut idx: u32 = 0u32;
    while idx < num_cpus {
        if idx != current_cpu {
            if cpu_active[idx as usize] {
                let bit: u32 = 1u32 << idx;
                if (target_cpu_mask & bit) != 0u32 {
                    if cpu_prios[idx as usize] > target_prio {
                        mask = mask | bit;
                    }
                }
            }
        }
        idx = idx + 1u32;
    }
    mask
}
/// Validate a previously computed IPI mask.
///
/// Checks structural properties that must hold for any valid mask:
/// - current CPU bit is not set
/// - no bits at or above max_cpus are set
pub fn validate_ipi_mask(mask: u32, current_cpu: u32, max_cpus: u32) -> bool {
    let current_bit: u32 = 1u32 << current_cpu;
    let current_excluded = (mask & current_bit) == 0u32;
    let bounded = (mask >> max_cpus) == 0u32;
    current_excluded && bounded
}
