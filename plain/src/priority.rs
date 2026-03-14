//! Bounded priority type for Zephyr thread scheduling.
//!
//! Zephyr uses lower numerical values for higher priority.
//! Priority 0 is the highest; MAX_PRIORITY-1 is the lowest.
//! This maps to Zephyr's CONFIG_NUM_COOP_PRIORITIES + CONFIG_NUM_PREEMPT_PRIORITIES.

//! Bounded priority type for Zephyr thread scheduling.
//!
//! Zephyr uses lower numerical values for higher priority.
//! Priority 0 is the highest; MAX_PRIORITY-1 is the lowest.
//! This maps to Zephyr's CONFIG_NUM_COOP_PRIORITIES + CONFIG_NUM_PREEMPT_PRIORITIES.
/// Maximum number of priority levels. Configurable per-system.
/// Default matches Zephyr: 16 cooperative + 16 preemptive = 32.
pub const MAX_PRIORITY: u32 = 32;
/// A bounded thread priority.
///
/// Invariant: value < MAX_PRIORITY.
/// Lower value = higher scheduling priority (woken first from wait queues).
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Priority {
    pub value: u32,
}
impl Priority {
    /// Create a new priority. Fails if out of range.
    pub fn new(value: u32) -> Option<Self> {
        if value < MAX_PRIORITY { Some(Priority { value }) } else { None }
    }
    /// Get the raw priority value.
    pub fn get(&self) -> u32 {
        self.value
    }
    /// Returns true if self is higher priority (lower numerical value) than other.
    pub fn is_higher_than(&self, other: &Priority) -> bool {
        self.value < other.value
    }
    /// Returns true if self is higher or equal priority to other.
    pub fn is_higher_or_equal(&self, other: &Priority) -> bool {
        self.value <= other.value
    }
}
