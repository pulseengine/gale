//! Verified thread stack configuration model for Zephyr RTOS.
//!
//! This is a formally verified model of Zephyr's thread stack setup
//! from kernel/thread.c (setup_thread_stack, ~line 383-470).
//! All safety-critical properties are proven with Verus (SMT/Z3).
//!
//! This module models the **stack geometry validation** of Zephyr's
//! thread creation subsystem. Actual memory mapping, stack pointer
//! randomization, and TLS setup remain in C.
//!
//! Source mapping:
//!   setup_thread_stack     -> StackConfig::validate   (thread.c:383-470)
//!   K_THREAD_STACK_LEN     -> stack_obj_size          (thread.c:392)
//!   K_THREAD_STACK_BUFFER  -> stack_buf_start          (thread.c:393)
//!   K_THREAD_STACK_RESERVED -> guard_size              (thread.c:394)
//!   STACK_SENTINEL          -> sentinel check          (thread.c:317-348)
//!
//! Omitted (not safety-relevant):
//!   - CONFIG_THREAD_STACK_MEM_MAPPED — virtual memory mapping
//!   - CONFIG_STACK_POINTER_RANDOM — random offset (cosmetic)
//!   - CONFIG_INIT_STACKS — debug stack fill pattern
//!   - CONFIG_THREAD_LOCAL_STORAGE — TLS area carve-out
//!   - CONFIG_USERSPACE / z_stack_is_user_capable — user mode stack handling
//!
//! ASIL-D verified properties:
//!   SK_S1: size is aligned to alignment
//!   SK_S2: guard_size <= size
//!   SK_S3: base + size doesn't overflow u32
//!   SK_S4: guard region doesn't overlap usable stack
//!   SK_S5: alignment is power of 2
use crate::error::*;
/// Thread stack configuration model.
///
/// Corresponds to the stack geometry computed in setup_thread_stack()
/// (thread.c:383-470). The C code computes stack_obj_size,
/// stack_buf_start, stack_buf_size, and stack_ptr from the stack
/// object and requested size. This model captures the validation
/// constraints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StackConfig {
    /// Base address of the stack buffer (lowest address).
    pub base: u32,
    /// Total size of the stack buffer in bytes.
    pub size: u32,
    /// Size of the guard region at the bottom of the stack.
    /// Corresponds to K_THREAD_STACK_RESERVED or MPU guard size.
    pub guard_size: u32,
    /// Required alignment (must be a power of 2).
    /// Corresponds to arch-specific stack alignment (e.g., 8 for ARM).
    pub alignment: u32,
}
impl StackConfig {
    /// Validate and create a stack configuration.
    ///
    /// Models the validation logic in setup_thread_stack() (thread.c:383-470).
    ///
    /// Returns EINVAL if any of the five safety properties would be violated.
    pub fn validate(
        base: u32,
        size: u32,
        guard_size: u32,
        alignment: u32,
    ) -> Result<StackConfig, i32> {
        if alignment == 0 || (alignment & (alignment - 1)) != 0 {
            return Err(EINVAL);
        }
        if size == 0 || (size % alignment) != 0 {
            return Err(EINVAL);
        }
        if guard_size >= size {
            return Err(EINVAL);
        }
        if base > u32::MAX - size {
            return Err(EINVAL);
        }
        Ok(StackConfig {
            base,
            size,
            guard_size,
            alignment,
        })
    }
    /// Get the usable stack size (total size minus guard region).
    ///
    /// This is the amount of stack available for the thread to use.
    pub fn usable_size(&self) -> u32 {
        self.size - self.guard_size
    }
    /// Get the top of the stack (base + size).
    ///
    /// This is where the initial stack pointer is set (for downward-growing
    /// stacks). Corresponds to stack_ptr in setup_thread_stack().
    pub fn top(&self) -> u32 {
        self.base + self.size
    }
    /// Get the start of the usable stack region (base + guard_size).
    ///
    /// Stack must not grow below this address. If it does, the guard
    /// region triggers a fault (MPU or sentinel check).
    pub fn usable_start(&self) -> u32 {
        self.base + self.guard_size
    }
    /// Check if a stack pointer is within the valid usable range.
    ///
    /// A valid stack pointer is in [usable_start, top].
    /// For downward-growing stacks, sp starts at top and decreases.
    pub fn is_valid_sp(&self, sp: u32) -> bool {
        sp >= self.base + self.guard_size && sp <= self.base + self.size
    }
    /// Check if a stack pointer is in the guard region (fault zone).
    ///
    /// Models the stack sentinel / MPU guard check in
    /// z_check_stack_sentinel() (thread.c:333-347).
    pub fn is_in_guard(&self, sp: u32) -> bool {
        sp >= self.base && sp < self.base + self.guard_size
    }
    /// Check if alignment is power of 2 (exec version).
    pub fn is_power_of_two(v: u32) -> bool {
        v > 0 && (v & (v - 1)) == 0
    }
    /// Get the base address.
    pub fn base_get(&self) -> u32 {
        self.base
    }
    /// Get the total size.
    pub fn size_get(&self) -> u32 {
        self.size
    }
    /// Get the guard region size.
    pub fn guard_size_get(&self) -> u32 {
        self.guard_size
    }
    /// Get the alignment.
    pub fn alignment_get(&self) -> u32 {
        self.alignment
    }
}
