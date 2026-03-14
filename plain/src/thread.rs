//! Thread state machine for Zephyr kernel.
//!
//! Models the subset of Zephyr's k_thread relevant to synchronization primitives.
//! A thread has an identity, a static priority, and a state that transitions
//! through Ready -> Running -> Blocked -> Ready as it interacts with kernel objects.
//!
//! Corresponds to: zephyr/kernel/include/kthread.h, kernel/thread.c
use crate::priority::Priority;
/// Unique thread identifier.
/// In Zephyr this is the pointer to the k_thread struct;
/// here we use a simple index for verifiability.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ThreadId {
    pub id: u32,
}
impl ThreadId {}
/// Thread execution state.
///
/// Models the relevant subset of Zephyr's _THREAD_* flags.
/// Blocked carries a return_value that gets set when the thread is woken.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ThreadState {
    /// Thread is ready to run (in the ready queue).
    Ready,
    /// Thread is the currently executing thread.
    Running,
    /// Thread is blocked on a kernel object (semaphore, mutex, etc).
    /// Stores the return value that will be set when unblocked.
    Blocked,
    /// Thread is suspended (not schedulable until explicitly resumed).
    Suspended,
}
/// A minimal thread model for synchronization verification.
///
/// We intentionally omit: stack, arch context, thread options, swap_data,
/// timeout, and all fields not relevant to synchronization primitive correctness.
#[derive(Debug, Copy, Clone)]
pub struct Thread {
    /// Unique identifier.
    pub id: ThreadId,
    /// Static priority (lower value = higher priority).
    pub priority: Priority,
    /// Current execution state.
    pub state: ThreadState,
    /// Return value set by kernel when unblocking this thread.
    /// Corresponds to arch_thread_return_value_set() in Zephyr.
    pub return_value: i32,
}
impl Thread {
    /// Create a new thread in the Ready state.
    pub fn new(id: u32, priority: Priority) -> Self {
        Thread {
            id: ThreadId { id },
            priority,
            state: ThreadState::Ready,
            return_value: 0,
        }
    }
    /// Transition: Ready -> Running (scheduler dispatches this thread).
    pub fn dispatch(&mut self) {
        self.state = ThreadState::Running;
    }
    /// Transition: Running -> Blocked (thread pends on a kernel object).
    /// Corresponds to z_pend_curr() in Zephyr.
    pub fn block(&mut self) {
        self.state = ThreadState::Blocked;
    }
    /// Transition: Blocked -> Ready (kernel object wakes this thread).
    /// Corresponds to z_ready_thread() + arch_thread_return_value_set().
    pub fn wake(&mut self, return_value: i32) {
        self.return_value = return_value;
        self.state = ThreadState::Ready;
    }
    /// Check if thread is blocked.
    pub fn is_blocked(&self) -> bool {
        matches!(self.state, ThreadState::Blocked)
    }
}
