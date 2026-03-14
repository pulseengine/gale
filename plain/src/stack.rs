//! Plain Rust LIFO stack for testing and Rocq-of-Rust translation.
//!
//! Identical logic to the Verus-annotated src/stack.rs.
//! Any divergence between these files is a bug.
//!
//! Source mapping:
//!   k_stack_init   -> Stack::init   (stack.c:27-42)
//!   k_stack_push   -> Stack::push   (stack.c:101-136, capacity check + increment)
//!   k_stack_pop    -> Stack::pop    (stack.c:148-190, empty check + decrement)

use crate::error::{EBUSY, EINVAL, ENOMEM, OK};

/// LIFO stack — count/capacity model.
///
/// Models Zephyr's k_stack pointer arithmetic as simple count tracking.
/// count = (next - base), capacity = (top - base).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stack {
    capacity: u32,
    count: u32,
}

impl Stack {
    /// Initialize a stack with given capacity.
    ///
    /// stack.c:27-42
    pub fn init(capacity: u32) -> Result<Self, i32> {
        if capacity == 0 {
            return Err(EINVAL);
        }
        Ok(Stack { capacity, count: 0 })
    }

    /// Push an entry onto the stack.
    ///
    /// stack.c:109-125
    ///
    /// Returns OK (count incremented) or ENOMEM (full, unchanged).
    pub fn push(&mut self) -> i32 {
        if self.count >= self.capacity {
            return ENOMEM;
        }
        // Safe: count < capacity <= u32::MAX, so count+1 cannot overflow.
        #[allow(clippy::arithmetic_side_effects)]
        {
            self.count += 1;
        }
        OK
    }

    /// Pop an entry from the stack.
    ///
    /// stack.c:158-166
    ///
    /// Returns OK (count decremented) or EBUSY (empty, unchanged).
    pub fn pop(&mut self) -> i32 {
        if self.count == 0 {
            return EBUSY;
        }
        // Safe: count > 0, so count-1 >= 0.
        #[allow(clippy::arithmetic_side_effects)]
        {
            self.count -= 1;
        }
        OK
    }

    /// Number of free slots.
    pub fn num_free(&self) -> u32 {
        // Safe: count <= capacity (invariant).
        #[allow(clippy::arithmetic_side_effects)]
        let r = self.capacity - self.count;
        r
    }

    /// Number of used slots.
    pub fn num_used(&self) -> u32 {
        self.count
    }

    /// Stack is full.
    pub fn is_full(&self) -> bool {
        self.count == self.capacity
    }

    /// Stack is empty.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Capacity accessor.
    pub fn capacity(&self) -> u32 {
        self.capacity
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::wildcard_enum_match_arm,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
mod tests {
    use super::*;

    #[test]
    fn test_init_valid() {
        let s = Stack::init(10).unwrap();
        assert_eq!(s.num_used(), 0);
        assert_eq!(s.num_free(), 10);
        assert!(s.is_empty());
        assert!(!s.is_full());
    }

    #[test]
    fn test_init_zero_capacity() {
        assert_eq!(Stack::init(0), Err(EINVAL));
    }

    #[test]
    fn test_push_pop_basic() {
        let mut s = Stack::init(5).unwrap();
        assert_eq!(s.push(), OK);
        assert_eq!(s.num_used(), 1);
        assert_eq!(s.num_free(), 4);

        assert_eq!(s.pop(), OK);
        assert_eq!(s.num_used(), 0);
        assert!(s.is_empty());
    }

    #[test]
    fn test_push_full() {
        let mut s = Stack::init(2).unwrap();
        assert_eq!(s.push(), OK);
        assert_eq!(s.push(), OK);
        assert!(s.is_full());

        assert_eq!(s.push(), ENOMEM);
        assert!(s.is_full());
    }

    #[test]
    fn test_pop_empty() {
        let mut s = Stack::init(3).unwrap();
        assert_eq!(s.pop(), EBUSY);
        assert!(s.is_empty());
    }

    #[test]
    fn test_fill_drain() {
        let mut s = Stack::init(4).unwrap();
        for _ in 0..4 {
            assert_eq!(s.push(), OK);
        }
        assert!(s.is_full());
        assert_eq!(s.num_used(), 4);
        assert_eq!(s.num_free(), 0);

        for _ in 0..4 {
            assert_eq!(s.pop(), OK);
        }
        assert!(s.is_empty());
        assert_eq!(s.num_used(), 0);
        assert_eq!(s.num_free(), 4);
    }

    #[test]
    fn test_conservation() {
        let mut s = Stack::init(8).unwrap();
        for _ in 0..5 {
            s.push();
            assert_eq!(s.num_free() + s.num_used(), 8);
        }
        for _ in 0..3 {
            s.pop();
            assert_eq!(s.num_free() + s.num_used(), 8);
        }
    }

    #[test]
    fn test_capacity_one() {
        let mut s = Stack::init(1).unwrap();
        assert!(s.is_empty());
        assert!(!s.is_full());

        assert_eq!(s.push(), OK);
        assert!(s.is_full());
        assert!(!s.is_empty());

        assert_eq!(s.push(), ENOMEM);

        assert_eq!(s.pop(), OK);
        assert!(s.is_empty());
    }

    #[test]
    fn test_max_capacity() {
        let s = Stack::init(u32::MAX).unwrap();
        assert_eq!(s.capacity(), u32::MAX);
        assert_eq!(s.num_free(), u32::MAX);
    }
}
