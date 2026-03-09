//! Bounded priority type for Zephyr thread scheduling.
//!
//! Lower numerical value = higher scheduling priority.
//! Priority 0 is highest; MAX_PRIORITY-1 is lowest.

/// Maximum number of priority levels (16 cooperative + 16 preemptive).
pub const MAX_PRIORITY: u32 = 32;

/// A bounded thread priority.
///
/// Invariant: value < MAX_PRIORITY.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Priority {
    pub value: u32,
}

impl Priority {
    /// Create a new priority. Returns None if out of range.
    pub fn new(value: u32) -> Option<Self> {
        if value < MAX_PRIORITY {
            Some(Priority { value })
        } else {
            None
        }
    }

    /// Get the raw priority value.
    pub fn get(&self) -> u32 {
        self.value
    }

    /// Returns true if self has higher priority (lower value) than other.
    pub fn is_higher_than(&self, other: &Priority) -> bool {
        self.value < other.value
    }

    /// Returns true if self has higher or equal priority to other.
    pub fn is_higher_or_equal(&self, other: &Priority) -> bool {
        self.value <= other.value
    }
}

impl PartialOrd for Priority {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Priority {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        // Lower value = higher priority, so reverse the ordering
        self.value.cmp(&other.value)
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
    fn test_priority_bounds() {
        assert!(Priority::new(0).is_some());
        assert!(Priority::new(MAX_PRIORITY - 1).is_some());
        assert!(Priority::new(MAX_PRIORITY).is_none());
        assert!(Priority::new(u32::MAX).is_none());
    }

    #[test]
    fn test_priority_ordering() {
        let high = Priority::new(0).unwrap();
        let low = Priority::new(31).unwrap();
        assert!(high.is_higher_than(&low));
        assert!(!low.is_higher_than(&high));
        assert!(high.is_higher_or_equal(&high));
    }
}
