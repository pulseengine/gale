//! Property-based tests for the scheduler primitives.
//!
//! Uses proptest to generate random operation sequences and verify that
//! ASIL-D invariants are maintained across RunQueue, SchedChoice, and the
//! preemption decision functions.
//!
//! Properties tested:
//!   SC1:  best() returns the highest-priority (lowest numeric) thread
//!   SC2:  add preserves sorted ordering
//!   SC3:  remove_best preserves sorted ordering
//!   SC5:  next_up always returns highest-priority eligible thread
//!   SC6:  cooperative threads are not preempted by non-MetaIRQ
//!   SC7:  idle selected only when no ready threads exist
//!   SC13: Dead is terminal — no outgoing transitions
//!   SC14: suspend is idempotent
//!   SC15: resume only from Suspended

#![allow(
    clippy::shadow_unrelated,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::wildcard_enum_match_arm
)]

use gale::priority::{Priority, MAX_PRIORITY};
use gale::sched::{
    CpuSchedState, RunQueue, SchedChoice, SchedThreadState, next_up, prio_cmp,
    sched_abort, sched_is_valid_transition, sched_pend, sched_resume, sched_sleep,
    sched_suspend, sched_unpend, sched_wakeup, should_preempt,
};
use gale::thread::Thread;
use proptest::prelude::*;

// =====================================================================
// Strategies
// =====================================================================

/// Generate a valid thread priority in [0, MAX_PRIORITY).
fn priority_strategy() -> impl Strategy<Value = u32> {
    0u32..MAX_PRIORITY
}

/// Generate a ready thread with given id and random priority.
fn thread_strategy(id: u32) -> impl Strategy<Value = Thread> {
    priority_strategy().prop_map(move |prio| {
        Thread::new(id, Priority::new(prio).unwrap())
    })
}

/// Generate a ready thread with a specific id and specific priority.
fn thread_with_prio(id: u32, prio: u32) -> Thread {
    Thread::new(id, Priority::new(prio).unwrap())
}

/// Operations that can be performed on a RunQueue.
#[derive(Debug, Clone)]
enum RunQueueOp {
    Add { id: u32, priority: u32 },
    RemoveBest,
    Best,
    IsEmpty,
    Count,
}

fn runqueue_op_strategy() -> impl Strategy<Value = RunQueueOp> {
    prop_oneof![
        // Weighted toward Add so the queue fills up meaningfully.
        3 => (1u32..=1000, priority_strategy()).prop_map(|(id, priority)| {
            RunQueueOp::Add { id, priority }
        }),
        2 => Just(RunQueueOp::RemoveBest),
        1 => Just(RunQueueOp::Best),
        1 => Just(RunQueueOp::IsEmpty),
        1 => Just(RunQueueOp::Count),
    ]
}

/// All variants of SchedThreadState for FSM tests.
fn sched_thread_state_strategy() -> impl Strategy<Value = SchedThreadState> {
    prop_oneof![
        Just(SchedThreadState::Ready),
        Just(SchedThreadState::Running),
        Just(SchedThreadState::Pending),
        Just(SchedThreadState::Suspended),
        Just(SchedThreadState::Sleeping),
        Just(SchedThreadState::Dead),
        Just(SchedThreadState::Aborting),
    ]
}

// =====================================================================
// Helper: assert RunQueue sorted invariant
// =====================================================================

/// Return all thread priorities currently in the queue, in index order.
fn extract_priorities(rq: &RunQueue) -> Vec<u32> {
    let mut prios = Vec::new();
    for i in 0..rq.len as usize {
        prios.push(rq.entries[i].unwrap().priority.get());
    }
    prios
}

/// Check that a slice is non-decreasing (ascending numeric = descending
/// scheduling priority, which is the sorted order).
fn is_sorted_ascending(v: &[u32]) -> bool {
    v.windows(2).all(|w| w[0] <= w[1])
}

// =====================================================================
// RunQueue invariants
// =====================================================================

proptest! {
    /// SC2 + SC3: sorted ordering is preserved after any sequence of
    /// add / remove_best operations.
    #[test]
    fn runq_sorted_invariant_after_random_ops(
        ops in prop::collection::vec(runqueue_op_strategy(), 0..150),
    ) {
        let mut rq = RunQueue::new();

        for op in ops {
            match op {
                RunQueueOp::Add { id, priority } => {
                    if rq.count() < 64 {
                        let t = thread_with_prio(id, priority);
                        let _ = rq.add(t);
                    }
                }
                RunQueueOp::RemoveBest => {
                    let _ = rq.remove_best();
                }
                RunQueueOp::Best => {
                    let _ = rq.best();
                }
                RunQueueOp::IsEmpty => {
                    let _ = rq.is_empty();
                }
                RunQueueOp::Count => {
                    let _ = rq.count();
                }
            }

            // INVARIANT: entries are always sorted ascending by priority value.
            let prios = extract_priorities(&rq);
            prop_assert!(
                is_sorted_ascending(&prios),
                "queue not sorted after op: prios={prios:?}"
            );

            // INVARIANT: len matches actual occupied entries.
            prop_assert_eq!(rq.count() as usize, prios.len());
        }
    }

    /// SC1: best() always returns the minimum priority value present.
    #[test]
    fn runq_best_always_highest_priority(
        priorities in prop::collection::vec(priority_strategy(), 1..30),
    ) {
        let mut rq = RunQueue::new();
        let mut expected_min = u32::MAX;

        for (i, &prio) in priorities.iter().enumerate() {
            if rq.count() < 64 {
                let t = thread_with_prio(i as u32, prio);
                rq.add(t);
                if prio < expected_min {
                    expected_min = prio;
                }
            }
        }

        if rq.count() > 0 {
            let best = rq.best().unwrap();
            prop_assert_eq!(
                best.priority.get(),
                expected_min,
                "SC1: best() must return the lowest numeric priority"
            );
        } else {
            prop_assert!(rq.best().is_none());
        }
    }

    /// SC3: remove_best always returns threads in priority order (non-decreasing
    /// priority value — highest scheduling priority first).
    #[test]
    fn runq_remove_best_priority_order(
        priorities in prop::collection::vec(priority_strategy(), 1..30),
    ) {
        let mut rq = RunQueue::new();
        for (i, &prio) in priorities.iter().enumerate() {
            if rq.count() < 64 {
                rq.add(thread_with_prio(i as u32, prio));
            }
        }

        let mut prev_prio = 0u32;
        while let Some(t) = rq.remove_best() {
            prop_assert!(
                t.priority.get() >= prev_prio,
                "SC3: remove_best must return in non-decreasing priority-value order \
                 (got {} after {})", t.priority.get(), prev_prio
            );
            prev_prio = t.priority.get();
        }
        prop_assert!(rq.is_empty());
    }

    /// SC7: idle is only selected when runq is empty.
    #[test]
    fn next_up_idle_only_when_empty(
        idle_prio in priority_strategy(),
        runq_prio in prop::option::of(priority_strategy()),
    ) {
        let idle = thread_with_prio(0, idle_prio);
        let runq_best = runq_prio.map(|p| thread_with_prio(1, p));
        let choice = next_up(runq_best, idle);

        match (runq_best, &choice) {
            (Some(_), SchedChoice::Thread(chosen)) => {
                // When there is a runq thread, next_up must not return idle.
                prop_assert!(
                    chosen.id.id != idle.id.id || runq_prio == Some(idle_prio),
                    "SC7: idle must not be chosen when runq has a thread"
                );
            }
            (None, SchedChoice::Thread(chosen)) => {
                // No runq thread — must select idle.
                prop_assert_eq!(
                    chosen.id.id,
                    idle.id.id,
                    "SC7: idle must be selected when runq is empty"
                );
            }
            (_, SchedChoice::Idle) => {
                // Bare Idle variant — only valid when runq is empty.
                prop_assert!(
                    runq_best.is_none(),
                    "SC7: Idle must only be returned when runq is empty"
                );
            }
        }
    }

    /// SC5: next_up returns the runq thread (highest priority) when available.
    #[test]
    fn next_up_returns_runq_thread_when_present(
        runq_prio in priority_strategy(),
        idle_prio in priority_strategy(),
    ) {
        let idle = thread_with_prio(0, idle_prio);
        let runq_thread = thread_with_prio(1, runq_prio);
        let choice = next_up(Some(runq_thread), idle);

        match choice {
            SchedChoice::Thread(chosen) => {
                prop_assert_eq!(
                    chosen.id.id,
                    runq_thread.id.id,
                    "SC5: runq thread must be chosen over idle"
                );
                prop_assert_eq!(
                    chosen.priority.get(),
                    runq_prio,
                    "SC5: returned thread must have the runq priority"
                );
            }
            SchedChoice::Idle => {
                prop_assert!(false, "SC5: Idle must never be returned when runq has a thread");
            }
        }
    }

    /// SC6: cooperative threads are NOT preempted by non-MetaIRQ candidates.
    #[test]
    fn should_preempt_cooperative_protection(
        swap_ok in proptest::bool::ANY,
    ) {
        // cooperative=true, candidate not MetaIRQ, no swap_ok
        let result = should_preempt(true, false, false);
        prop_assert!(
            !result,
            "SC6: cooperative thread must not be preempted by non-MetaIRQ"
        );
        let _ = swap_ok; // exercised in a separate test
    }

    /// SC6: swap_ok overrides cooperative protection.
    #[test]
    fn should_preempt_swap_ok_overrides_cooperative(
        candidate_is_metairq in proptest::bool::ANY,
    ) {
        let result = should_preempt(true, candidate_is_metairq, true);
        prop_assert!(result, "swap_ok must always allow preemption");
    }

    /// SC6: MetaIRQ can always preempt cooperative threads.
    #[test]
    fn should_preempt_metairq_preempts_cooperative(
        swap_ok in proptest::bool::ANY,
    ) {
        let result = should_preempt(true, true, swap_ok);
        prop_assert!(result, "MetaIRQ must always preempt cooperative threads");
    }

    /// prio_cmp is antisymmetric: cmp(a, b) = -cmp(b, a).
    #[test]
    fn prio_cmp_antisymmetric(
        pa in priority_strategy(),
        pb in priority_strategy(),
    ) {
        let a = thread_with_prio(0, pa);
        let b = thread_with_prio(1, pb);
        prop_assert_eq!(
            prio_cmp(&a, &b),
            -prio_cmp(&b, &a),
            "prio_cmp must be antisymmetric"
        );
    }

    /// prio_cmp is zero when priorities are equal.
    #[test]
    fn prio_cmp_zero_for_equal_priority(
        prio in priority_strategy(),
    ) {
        let a = thread_with_prio(0, prio);
        let b = thread_with_prio(1, prio);
        prop_assert_eq!(prio_cmp(&a, &b), 0, "equal priorities must compare as 0");
    }
}

// =====================================================================
// RunQueue count / empty coherence
// =====================================================================

proptest! {
    /// is_empty() is consistent with count() == 0 at all times.
    #[test]
    fn runq_empty_count_coherence(
        ops in prop::collection::vec(runqueue_op_strategy(), 0..100),
    ) {
        let mut rq = RunQueue::new();

        for op in ops {
            match op {
                RunQueueOp::Add { id, priority } => {
                    if rq.count() < 64 {
                        rq.add(thread_with_prio(id, priority));
                    }
                }
                RunQueueOp::RemoveBest => {
                    let _ = rq.remove_best();
                }
                _ => {}
            }

            prop_assert_eq!(
                rq.is_empty(),
                rq.count() == 0,
                "is_empty() must be consistent with count() == 0"
            );
        }
    }

    /// Draining a queue fully empties it.
    #[test]
    fn runq_drain_reaches_empty(
        priorities in prop::collection::vec(priority_strategy(), 0..30),
    ) {
        let mut rq = RunQueue::new();
        let added = priorities.len().min(64);
        for (i, &prio) in priorities.iter().take(added).enumerate() {
            rq.add(thread_with_prio(i as u32, prio));
        }
        let count_before = rq.count() as usize;
        let mut removed = 0usize;
        while rq.remove_best().is_some() {
            removed = removed.saturating_add(1);
        }
        prop_assert_eq!(removed, count_before, "drain must remove exactly count() threads");
        prop_assert!(rq.is_empty(), "queue must be empty after drain");
        prop_assert!(rq.best().is_none(), "best() must be None after drain");
    }

    /// add returns false when the queue is full (64 threads).
    #[test]
    fn runq_add_returns_false_when_full(
        extra_prio in priority_strategy(),
    ) {
        let mut rq = RunQueue::new();
        // Fill to capacity.
        for i in 0..64u32 {
            let ok = rq.add(thread_with_prio(i, 0));
            prop_assert!(ok, "add must succeed while below capacity");
        }
        prop_assert_eq!(rq.count(), 64);
        // One more must fail.
        let overflow = rq.add(thread_with_prio(999, extra_prio));
        prop_assert!(!overflow, "add must return false when queue is full");
        prop_assert_eq!(rq.count(), 64, "count must not change on failed add");
    }
}

// =====================================================================
// SchedThreadState FSM properties
// =====================================================================

proptest! {
    /// SC13: Dead is terminal — every transition from Dead returns Err.
    #[test]
    fn sched_dead_is_terminal(to in sched_thread_state_strategy()) {
        prop_assert!(
            !sched_is_valid_transition(SchedThreadState::Dead, to),
            "SC13: Dead -> {:?} must be invalid", to
        );
    }

    /// SC14: suspend is idempotent — suspending an already-Suspended thread
    /// succeeds and leaves the state as Suspended.
    #[test]
    fn sched_suspend_idempotent(_unused in 0u32..1) {
        let result = sched_suspend(SchedThreadState::Suspended);
        prop_assert!(result.is_ok(), "SC14: suspend(Suspended) must succeed");
        prop_assert_eq!(
            result.unwrap(),
            SchedThreadState::Suspended,
            "SC14: suspend(Suspended) must remain Suspended"
        );
    }

    /// SC15: resume only succeeds from Suspended.
    #[test]
    fn sched_resume_only_from_suspended(state in sched_thread_state_strategy()) {
        let result = sched_resume(state);
        if state == SchedThreadState::Suspended {
            prop_assert!(result.is_ok(), "SC15: resume(Suspended) must succeed");
            prop_assert_eq!(result.unwrap(), SchedThreadState::Ready);
        } else {
            prop_assert!(
                result.is_err(),
                "SC15: resume({:?}) must fail (only Suspended allowed)", state
            );
        }
    }

    /// sched_suspend: valid from Running, Pending, Suspended; invalid elsewhere.
    #[test]
    fn sched_suspend_valid_states(state in sched_thread_state_strategy()) {
        let result = sched_suspend(state);
        match state {
            SchedThreadState::Running
            | SchedThreadState::Pending
            | SchedThreadState::Suspended => {
                prop_assert!(result.is_ok(), "suspend must succeed from {:?}", state);
                prop_assert_eq!(result.unwrap(), SchedThreadState::Suspended);
            }
            _ => {
                prop_assert!(result.is_err(), "suspend must fail from {:?}", state);
            }
        }
    }

    /// sched_abort: fails only from Dead / Aborting; succeeds from all others.
    #[test]
    fn sched_abort_only_fails_from_dead_or_aborting(
        state in sched_thread_state_strategy(),
        smp_remote in proptest::bool::ANY,
    ) {
        let result = sched_abort(state, smp_remote);
        match state {
            SchedThreadState::Dead | SchedThreadState::Aborting => {
                prop_assert!(result.is_err(), "SC16: abort must fail from {:?}", state);
            }
            _ => {
                prop_assert!(result.is_ok(), "SC16: abort must succeed from {:?}", state);
            }
        }
    }

    /// sched_sleep: only valid from Running.
    #[test]
    fn sched_sleep_only_from_running(state in sched_thread_state_strategy()) {
        let result = sched_sleep(state);
        if state == SchedThreadState::Running {
            prop_assert!(result.is_ok(), "sleep must succeed from Running");
            prop_assert_eq!(result.unwrap(), SchedThreadState::Sleeping);
        } else {
            prop_assert!(result.is_err(), "sleep must fail from {:?}", state);
        }
    }

    /// sched_wakeup: only valid from Sleeping.
    #[test]
    fn sched_wakeup_only_from_sleeping(state in sched_thread_state_strategy()) {
        let result = sched_wakeup(state);
        if state == SchedThreadState::Sleeping {
            prop_assert!(result.is_ok(), "wakeup must succeed from Sleeping");
            prop_assert_eq!(result.unwrap(), SchedThreadState::Ready);
        } else {
            prop_assert!(result.is_err(), "wakeup must fail from {:?}", state);
        }
    }

    /// sched_pend: only valid from Running.
    #[test]
    fn sched_pend_only_from_running(state in sched_thread_state_strategy()) {
        let result = sched_pend(state);
        if state == SchedThreadState::Running {
            prop_assert!(result.is_ok(), "pend must succeed from Running");
            prop_assert_eq!(result.unwrap(), SchedThreadState::Pending);
        } else {
            prop_assert!(result.is_err(), "pend must fail from {:?}", state);
        }
    }

    /// sched_unpend: only valid from Pending.
    #[test]
    fn sched_unpend_only_from_pending(state in sched_thread_state_strategy()) {
        let result = sched_unpend(state);
        if state == SchedThreadState::Pending {
            prop_assert!(result.is_ok(), "unpend must succeed from Pending");
            prop_assert_eq!(result.unwrap(), SchedThreadState::Ready);
        } else {
            prop_assert!(result.is_err(), "unpend must fail from {:?}", state);
        }
    }

    /// suspend -> resume roundtrip returns to Ready.
    #[test]
    fn sched_suspend_resume_roundtrip(
        state in prop_oneof![
            Just(SchedThreadState::Running),
            Just(SchedThreadState::Pending),
        ],
    ) {
        let suspended = sched_suspend(state).unwrap();
        prop_assert_eq!(suspended, SchedThreadState::Suspended);
        let resumed = sched_resume(suspended).unwrap();
        prop_assert_eq!(resumed, SchedThreadState::Ready);
    }

    /// sleep -> wakeup roundtrip returns to Ready.
    #[test]
    fn sched_sleep_wakeup_roundtrip(_unused in 0u32..1) {
        let sleeping = sched_sleep(SchedThreadState::Running).unwrap();
        prop_assert_eq!(sleeping, SchedThreadState::Sleeping);
        let awake = sched_wakeup(sleeping).unwrap();
        prop_assert_eq!(awake, SchedThreadState::Ready);
    }

    /// pend -> unpend roundtrip returns to Ready.
    #[test]
    fn sched_pend_unpend_roundtrip(_unused in 0u32..1) {
        let pending = sched_pend(SchedThreadState::Running).unwrap();
        prop_assert_eq!(pending, SchedThreadState::Pending);
        let ready = sched_unpend(pending).unwrap();
        prop_assert_eq!(ready, SchedThreadState::Ready);
    }
}

// =====================================================================
// CpuSchedState construction
// =====================================================================

proptest! {
    /// A freshly created CpuSchedState has no MetaIRQ-preempted thread
    /// and swap_ok is false.
    #[test]
    fn cpu_sched_state_new_defaults(idle_prio in priority_strategy()) {
        let idle = thread_with_prio(0, idle_prio);
        let cpu = CpuSchedState::new(idle);
        prop_assert!(cpu.metairq_preempted.is_none(), "no MetaIRQ preemption initially");
        prop_assert!(!cpu.swap_ok, "swap_ok must be false initially");
        prop_assert_eq!(
            cpu.idle_thread.id.id,
            idle.id.id,
            "idle_thread must match the supplied thread"
        );
    }
}

// =====================================================================
// should_preempt exhaustive properties
// =====================================================================

proptest! {
    /// should_preempt(_, _, swap_ok=true) always returns true.
    #[test]
    fn should_preempt_swap_ok_always_true(
        coop in proptest::bool::ANY,
        metairq in proptest::bool::ANY,
    ) {
        prop_assert!(
            should_preempt(coop, metairq, true),
            "swap_ok must always permit preemption regardless of other flags"
        );
    }

    /// should_preempt(coop=false, _, swap_ok=false) always returns true
    /// (non-cooperative threads may always be preempted).
    #[test]
    fn should_preempt_preemptive_thread_always_preempted(
        metairq in proptest::bool::ANY,
    ) {
        prop_assert!(
            should_preempt(false, metairq, false),
            "preemptive (non-cooperative) thread must always be preemptible"
        );
    }

    /// should_preempt exhaustive enumeration of all 8 combinations.
    #[test]
    fn should_preempt_exhaustive(
        coop in proptest::bool::ANY,
        metairq in proptest::bool::ANY,
        swap_ok in proptest::bool::ANY,
    ) {
        let result = should_preempt(coop, metairq, swap_ok);
        if swap_ok {
            prop_assert!(result, "swap_ok -> always preempt");
        } else if coop && !metairq {
            prop_assert!(!result, "SC6: coop + !metairq + !swap_ok -> no preempt");
        } else {
            prop_assert!(result, "non-coop or metairq -> always preempt");
        }
    }
}
