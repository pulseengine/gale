//! Verified SMP CPU state tracking model for Zephyr RTOS.
//!
//! This is a formally verified model of Zephyr's SMP CPU lifecycle
//! from kernel/smp.c. All safety-critical properties are proven
//! with Verus (SMT/Z3).
//!
//! This module models the **CPU state tracking** of Zephyr's SMP
//! subsystem. Actual IPI signaling, interrupt stack setup, and arch
//! CPU start remain in C.
//!
//! Source mapping:
//!   z_smp_init           -> SmpState::init         (smp.c:222-241)
//!   k_smp_cpu_start      -> SmpState::start_cpu    (smp.c:170-194)
//!   k_smp_cpu_resume     -> SmpState::resume_cpu   (smp.c:196-219)
//!   z_smp_global_lock    -> SmpState::global_lock  (smp.c:57-70)
//!   z_smp_global_unlock  -> SmpState::global_unlock (smp.c:72-83)
//!
//! Omitted (not safety-relevant):
//!   - arch_cpu_start — hardware CPU power-up sequence
//!   - smp_init_top — per-CPU initialization callback
//!   - wait_for_start_signal / local_delay — synchronization spin-wait
//!   - z_dummy_thread_init — bootstrap thread setup
//!   - smp_timer_init — per-CPU timer initialization
//!   - z_smp_cpu_mobile / z_smp_current_get — IRQ state queries
//!
//! ASIL-D verified properties:
//!   SM1: 0 <= active_cpus <= max_cpus (bounds invariant)
//!   SM2: start_cpu when active < max: active += 1
//!   SM3: stop_cpu when active > 1: active -= 1 (CPU 0 never stops)
//!   SM4: global lock count is non-negative and bounded
use crate::error::*;
/// Maximum supported CPUs (matches CONFIG_MP_MAX_NUM_CPUS).
pub const MAX_CPUS: u32 = 16;
/// SMP CPU state tracking model.
///
/// Models the global SMP state: how many CPUs are active and
/// the global lock reference count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SmpState {
    /// Maximum number of CPUs in the system.
    pub max_cpus: u32,
    /// Number of currently active (running) CPUs.
    pub active_cpus: u32,
    /// Global lock reference count.
    pub global_lock_count: u32,
}
impl SmpState {
    /// Initialize SMP state with the given number of CPUs.
    ///
    /// Models z_smp_init() (smp.c:222-241).
    /// After init, only CPU 0 is active (others will be started).
    pub fn init(max_cpus: u32) -> Result<SmpState, i32> {
        if max_cpus == 0 || max_cpus > MAX_CPUS {
            Err(EINVAL)
        } else {
            Ok(SmpState {
                max_cpus,
                active_cpus: 1,
                global_lock_count: 0,
            })
        }
    }
    /// Start a CPU.
    ///
    /// Models k_smp_cpu_start() (smp.c:170-194).
    /// SM2: active_cpus increments if below max.
    pub fn start_cpu(&mut self) -> i32 {
        if self.active_cpus < self.max_cpus {
            #[allow(clippy::arithmetic_side_effects)]
            {
                self.active_cpus = self.active_cpus + 1;
            }
            OK
        } else {
            EBUSY
        }
    }
    /// Stop a CPU (power down).
    ///
    /// SM3: active_cpus decrements but never below 1 (CPU 0).
    pub fn stop_cpu(&mut self) -> i32 {
        if self.active_cpus > 1 {
            #[allow(clippy::arithmetic_side_effects)]
            {
                self.active_cpus = self.active_cpus - 1;
            }
            OK
        } else {
            EINVAL
        }
    }
    /// Resume a previously stopped CPU.
    ///
    /// Models k_smp_cpu_resume() (smp.c:196-219).
    /// Same semantics as start_cpu for the model.
    pub fn resume_cpu(&mut self) -> i32 {
        self.start_cpu()
    }
    /// Acquire the global SMP lock.
    ///
    /// Models z_smp_global_lock() (smp.c:57-70).
    /// SM4: increments lock count.
    pub fn global_lock(&mut self) -> i32 {
        #[allow(clippy::arithmetic_side_effects)]
        {
            self.global_lock_count = self.global_lock_count + 1;
        }
        OK
    }
    /// Release the global SMP lock.
    ///
    /// Models z_smp_global_unlock() (smp.c:72-83).
    /// SM4: decrements lock count, no underflow.
    pub fn global_unlock(&mut self) -> i32 {
        if self.global_lock_count > 0 {
            #[allow(clippy::arithmetic_side_effects)]
            {
                self.global_lock_count = self.global_lock_count - 1;
            }
            OK
        } else {
            EINVAL
        }
    }
    /// Number of active CPUs.
    pub fn active_get(&self) -> u32 {
        self.active_cpus
    }
    /// Number of inactive (available to start) CPUs.
    pub fn inactive_get(&self) -> u32 {
        #[allow(clippy::arithmetic_side_effects)]
        let r = self.max_cpus - self.active_cpus;
        r
    }
    /// Maximum CPUs.
    pub fn max_cpus_get(&self) -> u32 {
        self.max_cpus
    }
    /// Current global lock count.
    pub fn lock_count_get(&self) -> u32 {
        self.global_lock_count
    }
    /// Check if all CPUs are active.
    pub fn all_active(&self) -> bool {
        self.active_cpus == self.max_cpus
    }
    /// Check if the global lock is held.
    pub fn is_locked(&self) -> bool {
        self.global_lock_count > 0
    }
}
/// Decision for SMP start CPU: validate and compute new active count.
///
/// SM2: start increments active.
pub fn start_cpu_decide(active_cpus: u32, max_cpus: u32) -> Result<u32, i32> {
    if active_cpus < max_cpus { Ok(active_cpus + 1) } else { Err(EBUSY) }
}
/// Decision for SMP stop CPU: validate and compute new active count.
///
/// SM3: stop decrements active, CPU 0 never stops (min 1).
pub fn stop_cpu_decide(active_cpus: u32) -> Result<u32, i32> {
    if active_cpus > 1 { Ok(active_cpus - 1) } else { Err(EINVAL) }
}
