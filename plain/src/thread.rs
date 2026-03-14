//! Thread state machine for Zephyr kernel.
//!
//! Models the subset of Zephyr's k_thread relevant to synchronization.
//! State machine: Ready -> Running -> Blocked -> Ready.

use crate::priority::Priority;

/// Unique thread identifier.
/// In Zephyr this is the pointer to the k_thread struct;
/// here we use a simple index for verifiability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreadId {
    pub id: u32,
}

/// Thread execution state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadState {
    Ready,
    Running,
    Blocked,
    Suspended,
}

/// Minimal thread model for synchronization verification.
#[derive(Debug, Clone)]
pub struct Thread {
    pub id: ThreadId,
    pub priority: Priority,
    pub state: ThreadState,
    pub return_value: i32,
}

impl Thread {
    pub fn new(id: u32, priority: Priority) -> Self {
        Thread {
            id: ThreadId { id },
            priority,
            state: ThreadState::Ready,
            return_value: 0,
        }
    }

    /// Ready -> Running.
    pub fn dispatch(&mut self) {
        debug_assert_eq!(self.state, ThreadState::Ready);
        self.state = ThreadState::Running;
    }

    /// Running -> Blocked.
    pub fn block(&mut self) {
        debug_assert_eq!(self.state, ThreadState::Running);
        self.state = ThreadState::Blocked;
    }

    /// Blocked -> Ready, with a return value.
    pub fn wake(&mut self, return_value: i32) {
        debug_assert_eq!(self.state, ThreadState::Blocked);
        self.return_value = return_value;
        self.state = ThreadState::Ready;
    }

    pub fn is_blocked(&self) -> bool {
        self.state == ThreadState::Blocked
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
    fn test_state_machine_happy_path() {
        let p = Priority::new(5).unwrap();
        let mut t = Thread::new(1, p);
        assert_eq!(t.state, ThreadState::Ready);

        t.dispatch();
        assert_eq!(t.state, ThreadState::Running);

        t.block();
        assert_eq!(t.state, ThreadState::Blocked);
        assert!(t.is_blocked());

        t.wake(42);
        assert_eq!(t.state, ThreadState::Ready);
        assert_eq!(t.return_value, 42);
    }

    #[test]
    fn test_new_thread_defaults() {
        let p = Priority::new(0).unwrap();
        let t = Thread::new(99, p);
        assert_eq!(t.id.id, 99);
        assert_eq!(t.return_value, 0);
        assert_eq!(t.state, ThreadState::Ready);
    }
}
