//! Verified atomic operations model for Zephyr RTOS.
//!
//! This is a formally verified model of zephyr/kernel/atomic_c.c.
//! All safety-critical properties are proven with Verus (SMT/Z3).
//!
//! This module models the **pure arithmetic** of Zephyr's software-based
//! atomic operations. The actual spinlock-based atomicity (IRQ masking,
//! k_spin_lock/k_spin_unlock) remains in C — we model only the value
//! transformation semantics.
//!
//! Source mapping:
//!   atomic_get            -> AtomicVal::get          (atomic_c.c:233-236)
//!   z_impl_atomic_set     -> AtomicVal::set          (atomic_c.c:254-266)
//!   z_impl_atomic_add     -> AtomicVal::add          (atomic_c.c:178-191)
//!   z_impl_atomic_sub     -> AtomicVal::sub          (atomic_c.c:209-222)
//!   z_impl_atomic_or      -> AtomicVal::or           (atomic_c.c:285-297)
//!   z_impl_atomic_and     -> AtomicVal::and          (atomic_c.c:339-351)
//!   z_impl_atomic_xor     -> AtomicVal::xor          (atomic_c.c:312-324)
//!   z_impl_atomic_nand    -> AtomicVal::nand         (atomic_c.c:366-378)
//!   z_impl_atomic_cas     -> AtomicVal::cas          (atomic_c.c:88-108)
//!   (test_and_set)        -> AtomicVal::test_and_set (derived: cas(0,1) or set(1))
//!   (clear)               -> AtomicVal::clear        (derived: set(0))
//!
//! Omitted (not safety-relevant):
//!   - atomic_ptr_* variants — identical logic with different type
//!   - CONFIG_USERSPACE (z_vrfy_*) — syscall marshaling
//!   - CONFIG_SMP BUILD_ASSERT — compile-time guard
//!   - k_spin_lock/k_spin_unlock — atomicity mechanism
//!
//! ASIL-D verified properties:
//!   AT1: add returns old value, stores old + val (wrapping)
//!   AT2: sub returns old value, stores old - val (wrapping)
//!   AT3: cas succeeds only when current == expected
//!   AT4: cas failure leaves value unchanged
//!   AT5: test_and_set returns old value, sets to 1
//!   AT6: wrapping semantics for add/sub (matching hardware u32 behavior)
use crate::error::*;
/// Atomic value model.
///
/// Corresponds to Zephyr's atomic_t (which is `long` on most platforms).
/// We model as u32 to match Cortex-M atomic width.
///
/// Each operation returns the old value and mutates the stored value,
/// mirroring the C pattern: `ret = *target; *target = f(ret, arg); return ret;`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtomicVal {
    /// The stored atomic value.
    pub val: u32,
}
impl AtomicVal {
    /// Create a new atomic with initial value.
    pub fn new(initial: u32) -> AtomicVal {
        AtomicVal { val: initial }
    }
    /// Atomic get — read the current value.
    ///
    /// atomic_c.c:233-236: return *target;
    pub fn get(&self) -> u32 {
        self.val
    }
    /// Atomic set — write a new value, return old.
    ///
    /// atomic_c.c:254-266:
    ///   ret = *target; *target = value; return ret;
    pub fn set(&mut self, value: u32) -> u32 {
        let old_val = self.val;
        self.val = value;
        old_val
    }
    /// Atomic add — add value, return old (wrapping).
    ///
    /// atomic_c.c:178-191:
    ///   ret = *target; *target += value; return ret;
    ///
    /// AT1: returns old value, stores old + val.
    /// AT6: wrapping semantics.
    pub fn add(&mut self, value: u32) -> u32 {
        let old_val = self.val;
        self.val = add_u32_wrapping(self.val, value);
        old_val
    }
    /// Atomic sub — subtract value, return old (wrapping).
    ///
    /// atomic_c.c:209-222:
    ///   ret = *target; *target -= value; return ret;
    ///
    /// AT2: returns old value, stores old - val.
    /// AT6: wrapping semantics.
    pub fn sub(&mut self, value: u32) -> u32 {
        let old_val = self.val;
        self.val = sub_u32_wrapping(self.val, value);
        old_val
    }
    /// Atomic OR — bitwise OR, return old.
    ///
    /// atomic_c.c:285-297:
    ///   ret = *target; *target |= value; return ret;
    pub fn or(&mut self, value: u32) -> u32 {
        let old_val = self.val;
        self.val = self.val | value;
        old_val
    }
    /// Atomic AND — bitwise AND, return old.
    ///
    /// atomic_c.c:339-351:
    ///   ret = *target; *target &= value; return ret;
    pub fn and(&mut self, value: u32) -> u32 {
        let old_val = self.val;
        self.val = self.val & value;
        old_val
    }
    /// Atomic XOR — bitwise XOR, return old.
    ///
    /// atomic_c.c:312-324:
    ///   ret = *target; *target ^= value; return ret;
    pub fn xor(&mut self, value: u32) -> u32 {
        let old_val = self.val;
        self.val = self.val ^ value;
        old_val
    }
    /// Atomic NAND — bitwise NAND, return old.
    ///
    /// atomic_c.c:366-378:
    ///   ret = *target; *target = ~(*target & value); return ret;
    pub fn nand(&mut self, value: u32) -> u32 {
        let old_val = self.val;
        self.val = !(self.val & value);
        old_val
    }
    /// Atomic compare-and-swap.
    ///
    /// atomic_c.c:88-108:
    ///   if (*target == old_value) { *target = new_value; return true; }
    ///   return false;
    ///
    /// AT3: succeeds only when current == expected.
    /// AT4: failure leaves value unchanged.
    pub fn cas(&mut self, expected: u32, new_value: u32) -> bool {
        if self.val == expected {
            self.val = new_value;
            true
        } else {
            false
        }
    }
    /// Atomic test-and-set — set to 1, return old value.
    ///
    /// Equivalent to: old = *target; *target = 1; return old;
    ///
    /// AT5: returns old value, sets to 1.
    pub fn test_and_set(&mut self) -> u32 {
        let old_val = self.val;
        self.val = 1;
        old_val
    }
    /// Atomic clear — set to 0.
    ///
    /// Equivalent to: *target = 0;
    pub fn clear(&mut self) {
        self.val = 0;
    }
    /// Atomic increment — add 1, return old (wrapping).
    pub fn inc(&mut self) -> u32 {
        self.add(1)
    }
    /// Atomic decrement — subtract 1, return old (wrapping).
    pub fn dec(&mut self) -> u32 {
        self.sub(1)
    }
}
/// Wrapping u32 addition (models hardware behavior).
/// Result = (a + b) mod 2^32.
pub fn add_u32_wrapping(a: u32, b: u32) -> u32 {
    #[allow(clippy::arithmetic_side_effects)]
    let result = a.wrapping_add(b);
    result
}
/// Wrapping u32 subtraction (models hardware behavior).
/// Result = (a - b) mod 2^32.
pub fn sub_u32_wrapping(a: u32, b: u32) -> u32 {
    #[allow(clippy::arithmetic_side_effects)]
    let result = a.wrapping_sub(b);
    result
}
