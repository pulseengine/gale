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
use crate::thread::{Thread, ThreadId, ThreadState};
use crate::error::*;
