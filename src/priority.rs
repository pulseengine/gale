//! Bounded priority type for Zephyr thread scheduling.
//!
//! Zephyr uses lower numerical values for higher priority.
//! Priority 0 is the highest; MAX_PRIORITY-1 is the lowest.
//! This maps to Zephyr's CONFIG_NUM_COOP_PRIORITIES + CONFIG_NUM_PREEMPT_PRIORITIES.

use vstd::prelude::*;

verus! {

/// Maximum number of priority levels. Configurable per-system.
/// Default matches Zephyr: 16 cooperative + 16 preemptive = 32.
pub const MAX_PRIORITY: u32 = 32;

/// A bounded thread priority.
///
/// Invariant: value < MAX_PRIORITY.
/// Lower value = higher scheduling priority (woken first from wait queues).
#[derive(Copy, Clone)]
pub struct Priority {
    pub value: u32,
}

impl Priority {
    /// The representation invariant.
    pub open spec fn inv(&self) -> bool {
        self.value < MAX_PRIORITY
    }

    /// Ghost accessor for the priority value.
    pub open spec fn view(&self) -> nat {
        self.value as nat
    }

    /// Create a new priority. Fails if out of range.
    pub fn new(value: u32) -> (result: Option<Self>)
        ensures
            match result {
                Some(p) => p.inv() && p.value == value,
                None => value >= MAX_PRIORITY,
            },
    {
        if value < MAX_PRIORITY {
            Some(Priority { value })
        } else {
            None
        }
    }

    /// Get the raw priority value.
    pub fn get(&self) -> (result: u32)
        requires
            self.inv(),
        ensures
            result == self.value,
            result < MAX_PRIORITY,
    {
        self.value
    }

    /// Returns true if self is higher priority (lower numerical value) than other.
    pub fn is_higher_than(&self, other: &Priority) -> (result: bool)
        requires
            self.inv(),
            other.inv(),
        ensures
            result == (self.value < other.value),
    {
        self.value < other.value
    }

    /// Returns true if self is higher or equal priority to other.
    pub fn is_higher_or_equal(&self, other: &Priority) -> (result: bool)
        requires
            self.inv(),
            other.inv(),
        ensures
            result == (self.value <= other.value),
    {
        self.value <= other.value
    }
}

// === Proofs ===

/// Priority comparison is a total order.
pub proof fn lemma_priority_total_order(a: &Priority, b: &Priority)
    requires
        a.inv(),
        b.inv(),
    ensures
        a.value < b.value || a.value == b.value || a.value > b.value,
{
}

/// Transitivity of priority ordering.
pub proof fn lemma_priority_transitive(a: &Priority, b: &Priority, c: &Priority)
    requires
        a.inv(),
        b.inv(),
        c.inv(),
        a.value < b.value,
        b.value < c.value,
    ensures
        a.value < c.value,
{
}

} // verus!
