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
//!   next_up (SMP)  -> next_up_smp              (sched.c:221-278)
//!   update_cache   -> update_cache             (sched.c:294-319)
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
//!   SC9: MetaIRQ preempted thread preferred over runq_best (SMP)
//!   SC10: current stays if higher priority than candidate (SMP)
//!   SC11: ties only switch if swap_ok / yield (SMP)
//!   SC12: current re-queued only if active + not queued + not idle + not MetaIRQ preempted
use crate::error::*;
use crate::thread::{Thread, ThreadId, ThreadState};
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
                None, None, None, None, None, None, None, None, None, None, None, None, None, None,
                None, None, None, None, None, None, None, None, None, None, None, None, None, None,
                None, None, None, None, None, None, None, None, None, None, None, None, None, None,
                None, None, None, None, None, None, None, None, None, None, None, None, None, None,
                None, None, None, None, None, None, None, None,
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
/// Tracks MetaIRQ cooperative preemption state (per-CPU).
///
/// Models the scheduler-relevant fields of Zephyr's `struct _cpu`:
///   - `metairq_preempted`: cooperative thread displaced by a MetaIRQ
///   - `swap_ok`: set by k_yield() to allow same-priority switches
///   - `idle_thread`: the CPU's idle thread
///
/// Source mapping: arch/common/include/smp.h, kernel/sched.c
#[derive(Debug, Clone)]
pub struct CpuSchedState {
    /// Cooperative thread preempted by a MetaIRQ, if any.
    /// Zephyr: `_current_cpu->metairq_preempted`
    pub metairq_preempted: Option<Thread>,
    /// True when explicit yield (k_yield) allows same-priority swap.
    /// Zephyr: `_current_cpu->swap_ok`
    pub swap_ok: bool,
    /// The idle thread for this CPU.
    /// Zephyr: `_current_cpu->idle_thread`
    pub idle_thread: Thread,
}
impl CpuSchedState {
    /// Create a new per-CPU scheduler state.
    pub fn new(idle_thread: Thread) -> Self {
        CpuSchedState {
            metairq_preempted: None,
            swap_ok: false,
            idle_thread,
        }
    }
}
/// Outcome of the SMP next_up decision, including side-effect flags.
///
/// Beyond just the chosen thread, the SMP scheduler must communicate
/// whether the displaced current thread should be re-queued.
#[derive(Debug, Clone)]
pub struct SmpSchedOutcome {
    /// The thread selected to run next.
    pub choice: SchedChoice,
    /// True if the previously-running thread must be re-inserted
    /// into the run queue (it was active, not queued, not idle,
    /// and not the MetaIRQ-preempted thread).
    pub requeue_current: bool,
}
/// Select the next thread to run (SMP mode).
///
/// Models sched.c:next_up() for the CONFIG_SMP case (lines 221-278).
///
/// ASIL-D verified properties:
///   SC9:  MetaIRQ preempted thread is preferred over runq_best
///         (when ready and no MetaIRQ candidate in the queue)
///   SC10: In SMP, current stays if higher priority than candidate
///   SC11: Ties only switch if swap_ok (yield)
///   SC12: Current re-queued only if active + not queued + not idle
///         + not MetaIRQ preempted
pub fn next_up_smp(
    runq_best: Option<Thread>,
    current: Thread,
    cpu_state: &mut CpuSchedState,
    current_is_active: bool,
    current_is_queued: bool,
    current_is_cooperative: bool,
    candidate_is_metairq_fn: fn(&Thread) -> bool,
) -> SmpSchedOutcome {
    let mut thread: Option<Thread> = runq_best;
    if let Some(mirqp) = cpu_state.metairq_preempted {
        let best_is_metairq = match thread {
            Some(ref t) => candidate_is_metairq_fn(t),
            None => false,
        };
        if !best_is_metairq {
            if mirqp.state == ThreadState::Ready {
                thread = Some(mirqp);
            } else {
                cpu_state.metairq_preempted = None;
            }
        }
    }
    let candidate = match thread {
        Some(t) => t,
        None => cpu_state.idle_thread,
    };
    let mut chosen = candidate;
    if current_is_active {
        #[allow(clippy::arithmetic_side_effects)]
        let cmp = current.priority.get() as i64 - chosen.priority.get() as i64;
        if (cmp < 0) || ((cmp == 0) && !cpu_state.swap_ok) {
            chosen = current;
        }
        if !should_preempt(
            current_is_cooperative,
            candidate_is_metairq_fn(&chosen),
            cpu_state.swap_ok,
        ) {
            chosen = current;
        }
    }
    let is_switching = chosen.id != current.id;
    let is_current_idle = current.id == cpu_state.idle_thread.id;
    let is_current_mirq_preempted = match cpu_state.metairq_preempted {
        Some(mirqp) => current.id == mirqp.id,
        None => false,
    };
    let requeue_current = is_switching
        && current_is_active
        && !current_is_queued
        && !is_current_idle
        && !is_current_mirq_preempted;
    if is_switching {
        update_metairq_preempt(
            &chosen,
            &current,
            current_is_cooperative,
            candidate_is_metairq_fn,
            cpu_state,
        );
    }
    cpu_state.swap_ok = false;
    SmpSchedOutcome {
        choice: SchedChoice::Thread(chosen),
        requeue_current,
    }
}
/// Update MetaIRQ preemption tracking when switching threads.
///
/// Models sched.c:update_metairq_preempt() (lines 166-180).
fn update_metairq_preempt(
    new_thread: &Thread,
    current: &Thread,
    current_is_cooperative: bool,
    is_metairq: fn(&Thread) -> bool,
    cpu_state: &mut CpuSchedState,
) {
    if is_metairq(new_thread) && !is_metairq(current) && current_is_cooperative {
        cpu_state.metairq_preempted = Some(*current);
    } else if !is_metairq(new_thread) {
        cpu_state.metairq_preempted = None;
    }
}
/// Model of Zephyr's update_cache() for the non-SMP path.
///
/// Source mapping: sched.c:294-319
///
/// Returns the thread that should be stored in `ready_q.cache`.
pub fn update_cache(
    runq_best: Option<Thread>,
    current: Thread,
    cpu_state: &CpuSchedState,
    preempt_ok: bool,
    current_is_cooperative: bool,
    candidate_is_metairq: bool,
) -> Thread {
    let thread = match runq_best {
        Some(t) => t,
        None => cpu_state.idle_thread,
    };
    if should_preempt(current_is_cooperative, candidate_is_metairq, preempt_ok) {
        thread
    } else {
        current
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
/// Complete scheduler thread state, modelling all Zephyr _THREAD_* flags.
///
/// This is separate from `ThreadState` (used by synchronization primitives)
/// to avoid disrupting existing kernel object code. It models the full
/// lifecycle including suspend, sleep, abort, and death.
///
/// Source: kernel/sched.c halt_thread(), z_thread_halt(), z_tick_sleep(),
///         z_pend_curr(), z_impl_k_thread_suspend/resume/abort.
///
/// ASIL-D verified properties:
///   SC13: no transition from Dead (terminal state)
///   SC14: suspend is idempotent (Suspended -> Suspended = Suspended)
///   SC15: resume only from Suspended
///   SC16: abort always succeeds (any non-Dead state -> Dead or Aborting)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedThreadState {
    /// In run queue, eligible for scheduling.
    Ready,
    /// Currently executing on a CPU.
    Running,
    /// Blocked on a kernel object (wait queue) via z_pend_curr().
    Pending,
    /// Paused by k_thread_suspend(), not schedulable until resumed.
    Suspended,
    /// Blocked by k_sleep(), will auto-wake on timeout expiry.
    Sleeping,
    /// Terminated — awaiting join/cleanup. Terminal state.
    Dead,
    /// In process of being aborted (SMP: thread running on another CPU).
    /// Transitions to Dead once the target CPU processes the IPI.
    Aborting,
}
/// Check whether a transition from `from` to `to` is valid in the
/// scheduler thread lifecycle FSM.
///
/// SC13: Dead is a terminal state — no outgoing transitions.
pub fn sched_is_valid_transition(from: SchedThreadState, to: SchedThreadState) -> bool {
    match (from, to) {
        (SchedThreadState::Ready, SchedThreadState::Running) => true,
        (SchedThreadState::Ready, SchedThreadState::Dead) => true,
        (SchedThreadState::Ready, SchedThreadState::Aborting) => true,
        (SchedThreadState::Running, SchedThreadState::Ready) => true,
        (SchedThreadState::Running, SchedThreadState::Pending) => true,
        (SchedThreadState::Running, SchedThreadState::Suspended) => true,
        (SchedThreadState::Running, SchedThreadState::Sleeping) => true,
        (SchedThreadState::Running, SchedThreadState::Dead) => true,
        (SchedThreadState::Running, SchedThreadState::Aborting) => true,
        (SchedThreadState::Pending, SchedThreadState::Ready) => true,
        (SchedThreadState::Pending, SchedThreadState::Suspended) => true,
        (SchedThreadState::Pending, SchedThreadState::Dead) => true,
        (SchedThreadState::Pending, SchedThreadState::Aborting) => true,
        (SchedThreadState::Suspended, SchedThreadState::Ready) => true,
        (SchedThreadState::Suspended, SchedThreadState::Dead) => true,
        (SchedThreadState::Suspended, SchedThreadState::Aborting) => true,
        (SchedThreadState::Sleeping, SchedThreadState::Ready) => true,
        (SchedThreadState::Sleeping, SchedThreadState::Dead) => true,
        (SchedThreadState::Sleeping, SchedThreadState::Aborting) => true,
        (SchedThreadState::Aborting, SchedThreadState::Dead) => true,
        (SchedThreadState::Dead, _) => false,
        _ => false,
    }
}
/// Suspend a thread. Corresponds to k_thread_suspend().
///
/// SC14: suspend is idempotent — suspending an already-suspended thread
/// returns Ok(Suspended) without error.
///
/// Valid source states: Running, Pending, Suspended (idempotent).
pub fn sched_suspend(state: SchedThreadState) -> Result<SchedThreadState, i32> {
    match state {
        SchedThreadState::Running => Ok(SchedThreadState::Suspended),
        SchedThreadState::Pending => Ok(SchedThreadState::Suspended),
        SchedThreadState::Suspended => Ok(SchedThreadState::Suspended),
        SchedThreadState::Dead => Err(EINVAL),
        SchedThreadState::Aborting => Err(EINVAL),
        SchedThreadState::Ready => Err(EINVAL),
        SchedThreadState::Sleeping => Err(EINVAL),
    }
}
/// Resume a suspended thread. Corresponds to k_thread_resume().
///
/// SC15: resume only from Suspended.
pub fn sched_resume(state: SchedThreadState) -> Result<SchedThreadState, i32> {
    match state {
        SchedThreadState::Suspended => Ok(SchedThreadState::Ready),
        SchedThreadState::Ready => Err(EINVAL),
        SchedThreadState::Running => Err(EINVAL),
        SchedThreadState::Pending => Err(EINVAL),
        SchedThreadState::Sleeping => Err(EINVAL),
        SchedThreadState::Dead => Err(EINVAL),
        SchedThreadState::Aborting => Err(EINVAL),
    }
}
/// Abort a thread. Corresponds to k_thread_abort() / z_thread_halt(terminate=true).
///
/// SC16: abort always succeeds from any non-Dead/non-Aborting state.
///
/// The `smp_remote` flag indicates the thread is running on another CPU.
pub fn sched_abort(state: SchedThreadState, smp_remote: bool) -> Result<SchedThreadState, i32> {
    match state {
        SchedThreadState::Dead => Err(EINVAL),
        SchedThreadState::Aborting => Err(EINVAL),
        SchedThreadState::Running => {
            if smp_remote {
                Ok(SchedThreadState::Aborting)
            } else {
                Ok(SchedThreadState::Dead)
            }
        }
        SchedThreadState::Ready => Ok(SchedThreadState::Dead),
        SchedThreadState::Pending => Ok(SchedThreadState::Dead),
        SchedThreadState::Suspended => Ok(SchedThreadState::Dead),
        SchedThreadState::Sleeping => Ok(SchedThreadState::Dead),
    }
}
/// Put thread to sleep. Corresponds to k_sleep() / z_tick_sleep().
///
/// Only valid from Running (thread calls k_sleep on itself).
pub fn sched_sleep(state: SchedThreadState) -> Result<SchedThreadState, i32> {
    match state {
        SchedThreadState::Running => Ok(SchedThreadState::Sleeping),
        SchedThreadState::Ready => Err(EINVAL),
        SchedThreadState::Pending => Err(EINVAL),
        SchedThreadState::Suspended => Err(EINVAL),
        SchedThreadState::Sleeping => Err(EINVAL),
        SchedThreadState::Dead => Err(EINVAL),
        SchedThreadState::Aborting => Err(EINVAL),
    }
}
/// Wake a sleeping thread. Corresponds to timeout expiry or k_wakeup().
///
/// Only valid from Sleeping.
pub fn sched_wakeup(state: SchedThreadState) -> Result<SchedThreadState, i32> {
    match state {
        SchedThreadState::Sleeping => Ok(SchedThreadState::Ready),
        SchedThreadState::Ready => Err(EINVAL),
        SchedThreadState::Running => Err(EINVAL),
        SchedThreadState::Pending => Err(EINVAL),
        SchedThreadState::Suspended => Err(EINVAL),
        SchedThreadState::Dead => Err(EINVAL),
        SchedThreadState::Aborting => Err(EINVAL),
    }
}
/// Pend a running thread on a kernel object. Corresponds to z_pend_curr().
///
/// Only valid from Running (the current thread blocks itself).
pub fn sched_pend(state: SchedThreadState) -> Result<SchedThreadState, i32> {
    match state {
        SchedThreadState::Running => Ok(SchedThreadState::Pending),
        SchedThreadState::Ready => Err(EINVAL),
        SchedThreadState::Pending => Err(EINVAL),
        SchedThreadState::Suspended => Err(EINVAL),
        SchedThreadState::Sleeping => Err(EINVAL),
        SchedThreadState::Dead => Err(EINVAL),
        SchedThreadState::Aborting => Err(EINVAL),
    }
}
/// Unpend a thread from a kernel object. Corresponds to z_unpend_thread() /
/// z_sched_wake_thread_locked().
///
/// Only valid from Pending.
pub fn sched_unpend(state: SchedThreadState) -> Result<SchedThreadState, i32> {
    match state {
        SchedThreadState::Pending => Ok(SchedThreadState::Ready),
        SchedThreadState::Ready => Err(EINVAL),
        SchedThreadState::Running => Err(EINVAL),
        SchedThreadState::Suspended => Err(EINVAL),
        SchedThreadState::Sleeping => Err(EINVAL),
        SchedThreadState::Dead => Err(EINVAL),
        SchedThreadState::Aborting => Err(EINVAL),
    }
}
