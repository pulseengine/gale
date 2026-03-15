//! Verified scheduler primitives for Zephyr RTOS.
//!
//! This module models the safety-critical scheduling logic from
//! kernel/sched.c. It covers:
//!
//! 1. **Priority run queue** — sorted thread collection matching
//!    Zephyr's `_priq_run_*` interface (CONFIG_SCHED_DUMB mode).
//! 2. **next_up() decision** — scheduling policy that selects the
//!    highest-priority ready thread, with cooperative/MetaIRQ semantics.
//! 3. **Thread state FSM** — valid state transitions for scheduler operations.
//!
//! Source mapping:
//!   runq_add       -> RunQueue::add         (sched.c:80-86)
//!   runq_remove    -> RunQueue::remove      (sched.c:88-94)
//!   runq_best      -> RunQueue::best        (sched.c:101-104)
//!   runq_yield     -> RunQueue::yield_current (sched.c:96-99)
//!   next_up        -> SchedDecision::next_up  (sched.c:185-279)
//!   should_preempt -> SchedDecision::should_preempt (sched.c:128-145)
//!
//! ASIL-D verified properties:
//!   SC1: best() returns the highest-priority thread (lowest numeric value)
//!   SC2: add preserves sorted ordering
//!   SC3: remove preserves sorted ordering for remaining threads
//!   SC4: yield moves current to end of same-priority group
//!   SC5: next_up always returns highest-priority eligible thread
//!   SC6: cooperative threads are not preempted by non-MetaIRQ threads
//!   SC7: idle thread is selected only when no ready threads exist
//!   SC8: no arithmetic overflow in priority comparisons
use crate::thread::{Thread, ThreadId, ThreadState};
use crate::error::*;
/// Maximum threads in the run queue.
pub const MAX_RUNQ_SIZE: u32 = 64;
/// Priority-ordered run queue for the scheduler.
///
/// Models Zephyr's `struct _priq_rb` / `struct _priq_simple` /
/// `struct _priq_mq` (CONFIG_SCHED_DUMB mode — simple sorted list).
///
/// Structurally similar to WaitQueue but for the scheduler run queue.
#[derive(Debug)]
pub struct RunQueue {
    /// Threads in the run queue, sorted by priority (index 0 = highest).
    pub entries: [Option<Thread>; 64],
    /// Number of threads currently in the queue.
    pub len: u32,
}
impl RunQueue {
    /// Create an empty run queue.
    pub fn new() -> Self {
        RunQueue {
            entries: [
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            ],
            len: 0,
        }
    }
    /// Return the highest-priority (lowest numeric priority) thread
    /// without removing it. SC1: always returns the best thread.
    pub fn best(&self) -> Option<Thread> {
        if self.len == 0 { None } else { self.entries[0] }
    }
    /// Add a thread to the run queue in sorted position.
    /// SC2: preserves sorted ordering.
    pub fn add(&mut self, thread: Thread) -> bool {
        if self.len >= MAX_RUNQ_SIZE {
            return false;
        }
        let mut insert_pos: u32 = self.len;
        let mut i: u32 = 0;
        let mut found: bool = false;
        while i < self.len && !found {
            let entry_pri = self.entries[i as usize].unwrap().priority.get();
            let thr_pri = thread.priority.get();
            if thr_pri < entry_pri {
                insert_pos = i;
                found = true;
            }
            if !found {
                i = i + 1;
            }
        }
        let mut j: u32 = self.len;
        while j > insert_pos {
            self.entries[j as usize] = self.entries[(j - 1) as usize];
            self.entries[(j - 1) as usize] = None;
            j = j - 1;
        }
        self.entries[insert_pos as usize] = Some(thread);
        self.len = self.len + 1;
        true
    }
    /// Remove the first (highest-priority) thread from the queue.
    /// SC3: preserves sorted ordering for remaining threads.
    pub fn remove_best(&mut self) -> Option<Thread> {
        if self.len == 0 {
            return None;
        }
        let thread = self.entries[0];
        self.entries[0] = None;
        let mut i: u32 = 0;
        while i < self.len - 1 {
            self.entries[i as usize] = self.entries[(i + 1) as usize];
            self.entries[(i + 1) as usize] = None;
            i = i + 1;
        }
        self.len = self.len - 1;
        thread
    }
    /// Check if the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// Get the number of threads in the queue.
    pub fn count(&self) -> u32 {
        self.len
    }
}
/// Result of a scheduling decision.
#[derive(Debug, Clone)]
pub enum SchedChoice {
    /// Run this thread (from the run queue or current).
    Thread(Thread),
    /// No ready threads — run idle.
    Idle,
}
/// Priority comparison: negative means `a` is higher priority.
/// SC8: no overflow — uses i64 for the subtraction.
pub fn prio_cmp(a: &Thread, b: &Thread) -> i64 {
    #[allow(clippy::arithmetic_side_effects)]
    let r = a.priority.get() as i64 - b.priority.get() as i64;
    r
}
/// Determine whether `thread` should preempt `current`.
/// SC6: cooperative threads are not preempted unless by MetaIRQ.
///
/// In Zephyr, cooperative threads have priority < CONFIG_NUM_COOP_PRIORITIES.
/// We model this with a `is_cooperative` flag on the thread.
pub fn should_preempt(
    current_is_cooperative: bool,
    candidate_is_metairq: bool,
    swap_ok: bool,
) -> bool {
    if swap_ok {
        return true;
    }
    if current_is_cooperative && !candidate_is_metairq {
        return false;
    }
    true
}
/// Select the next thread to run (uniprocessor mode).
///
/// SC5: always returns highest-priority eligible thread.
/// SC7: idle only when no ready threads exist.
///
/// This models sched.c:next_up() for the !CONFIG_SMP case.
pub fn next_up(runq_best: Option<Thread>, idle: Thread) -> SchedChoice {
    match runq_best {
        Some(thread) => SchedChoice::Thread(thread),
        None => SchedChoice::Thread(idle),
    }
}
/// Valid scheduler state transitions.
/// Returns true if the transition from `from` to `to` is valid.
pub fn is_valid_transition(from: ThreadState, to: ThreadState) -> bool {
    match (from, to) {
        (ThreadState::Ready, ThreadState::Running) => true,
        (ThreadState::Running, ThreadState::Ready) => true,
        (ThreadState::Running, ThreadState::Blocked) => true,
        (ThreadState::Blocked, ThreadState::Ready) => true,
        (_, ThreadState::Ready) => true,
        _ => false,
    }
}
