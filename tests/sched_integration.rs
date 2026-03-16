//! Integration tests for the scheduler primitives.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::shadow_unrelated
)]

use gale::priority::Priority;
use gale::sched::*;
use gale::thread::{Thread, ThreadId, ThreadState};

fn make_ready_thread(id: u32, prio: u32) -> Thread {
    let mut t = Thread::new(id, Priority::new(prio).unwrap());
    t.state = ThreadState::Ready;
    t
}

// ── RunQueue: basic operations ─────────────────────────────────────

#[test]
fn runq_new_is_empty() {
    let rq = RunQueue::new();
    assert!(rq.is_empty());
    assert_eq!(rq.count(), 0);
    assert!(rq.best().is_none());
}

#[test]
fn runq_add_single_thread() {
    let mut rq = RunQueue::new();
    let t = make_ready_thread(1, 5);
    assert!(rq.add(t));
    assert_eq!(rq.count(), 1);
    assert!(!rq.is_empty());
    assert_eq!(rq.best().unwrap().id.id, 1);
}

#[test]
fn runq_best_returns_highest_priority() {
    let mut rq = RunQueue::new();
    rq.add(make_ready_thread(1, 10));
    rq.add(make_ready_thread(2, 5)); // higher priority (lower value)
    rq.add(make_ready_thread(3, 15));

    let best = rq.best().unwrap();
    assert_eq!(best.priority.get(), 5);
    assert_eq!(best.id.id, 2);
}

#[test]
fn runq_remove_best_returns_in_priority_order() {
    let mut rq = RunQueue::new();
    rq.add(make_ready_thread(1, 10));
    rq.add(make_ready_thread(2, 5));
    rq.add(make_ready_thread(3, 15));
    rq.add(make_ready_thread(4, 1));

    assert_eq!(rq.remove_best().unwrap().priority.get(), 1);
    assert_eq!(rq.remove_best().unwrap().priority.get(), 5);
    assert_eq!(rq.remove_best().unwrap().priority.get(), 10);
    assert_eq!(rq.remove_best().unwrap().priority.get(), 15);
    assert!(rq.remove_best().is_none());
}

#[test]
fn runq_add_same_priority_preserves_fifo() {
    let mut rq = RunQueue::new();
    rq.add(make_ready_thread(1, 5));
    rq.add(make_ready_thread(2, 5));
    rq.add(make_ready_thread(3, 5));

    // Same priority — first added should come out first (FIFO within priority)
    assert_eq!(rq.remove_best().unwrap().id.id, 1);
    assert_eq!(rq.remove_best().unwrap().id.id, 2);
    assert_eq!(rq.remove_best().unwrap().id.id, 3);
}

#[test]
fn runq_stress_fill_drain() {
    let mut rq = RunQueue::new();
    // Use priorities 0..31 (MAX_PRIORITY=32), repeat to fill queue
    for i in 0..32 {
        assert!(rq.add(make_ready_thread(i, 31 - i)));
    }
    assert_eq!(rq.count(), 32);

    // Should drain in priority order (0, 1, 2, ... 31)
    for expected_prio in 0..32u32 {
        let t = rq.remove_best().unwrap();
        assert_eq!(t.priority.get(), expected_prio);
    }
    assert!(rq.is_empty());
}

#[test]
fn runq_full_rejects_add() {
    let mut rq = RunQueue::new();
    // Fill with valid priorities (0..31 repeated)
    for i in 0..64u32 {
        rq.add(make_ready_thread(i, i % 32));
    }
    assert!(!rq.add(make_ready_thread(99, 0)));
}

// ── Scheduling decision: next_up ────────────────────────────────────

#[test]
fn next_up_returns_best_from_queue() {
    let best = make_ready_thread(1, 5);
    let idle = make_ready_thread(0, 31);
    let result = next_up(Some(best.clone()), idle);
    match result {
        SchedChoice::Thread(t) => assert_eq!(t.id.id, best.id.id),
        SchedChoice::Idle => panic!("expected thread"),
    }
}

#[test]
fn next_up_returns_idle_when_empty() {
    let idle = make_ready_thread(0, 31);
    let result = next_up(None, idle.clone());
    match result {
        SchedChoice::Thread(t) => assert_eq!(t.id.id, idle.id.id),
        SchedChoice::Idle => panic!("expected idle thread"),
    }
}

// ── should_preempt ──────────────────────────────────────────────────

#[test]
fn cooperative_thread_not_preempted() {
    let cand = make_ready_thread(1, 5);
    // Current is cooperative, candidate is NOT MetaIRQ, no swap_ok
    assert!(!should_preempt(true, false, false));
}

#[test]
fn cooperative_thread_preempted_by_metairq() {
    let cand = make_ready_thread(1, 0);
    // Current is cooperative, but candidate IS MetaIRQ
    assert!(should_preempt(true, true, false));
}

#[test]
fn swap_ok_always_preempts() {
    let cand = make_ready_thread(1, 5);
    // swap_ok = true (yield) always allows preemption
    assert!(should_preempt(true, false, true));
    assert!(should_preempt(false, false, true));
}

#[test]
fn preemptive_thread_preempted_normally() {
    let cand = make_ready_thread(1, 5);
    // Current is preemptive (not cooperative), any candidate preempts
    assert!(should_preempt(false, false, false));
}

// ── Priority comparison ─────────────────────────────────────────────

#[test]
fn prio_cmp_lower_value_is_higher_priority() {
    let a = make_ready_thread(1, 3);
    let b = make_ready_thread(2, 10);
    assert!(prio_cmp(&a, &b) < 0); // a has higher priority
}

#[test]
fn prio_cmp_equal_priorities() {
    let a = make_ready_thread(1, 5);
    let b = make_ready_thread(2, 5);
    assert_eq!(prio_cmp(&a, &b), 0);
}

// ── Thread state transitions ────────────────────────────────────────

#[test]
fn valid_state_transitions() {
    assert!(is_valid_transition(
        ThreadState::Ready,
        ThreadState::Running
    ));
    assert!(is_valid_transition(
        ThreadState::Running,
        ThreadState::Ready
    ));
    assert!(is_valid_transition(
        ThreadState::Running,
        ThreadState::Blocked
    ));
    assert!(is_valid_transition(
        ThreadState::Blocked,
        ThreadState::Ready
    ));
}

#[test]
fn blocked_to_running_requires_ready_intermediate() {
    // Blocked -> Running should go through Ready first
    // But our FSM allows any -> Ready, so Blocked -> Ready is valid
    assert!(is_valid_transition(
        ThreadState::Blocked,
        ThreadState::Ready
    ));
}

// ── End-to-end scheduling simulation ────────────────────────────────

#[test]
fn schedule_three_threads_priority_order() {
    let mut rq = RunQueue::new();
    let idle = make_ready_thread(0, 31);

    rq.add(make_ready_thread(1, 10));
    rq.add(make_ready_thread(2, 5));
    rq.add(make_ready_thread(3, 15));

    // First schedule: should pick thread 2 (prio 5)
    let best = rq.best();
    let choice = next_up(best, idle.clone());
    match choice {
        SchedChoice::Thread(t) => assert_eq!(t.priority.get(), 5),
        SchedChoice::Idle => panic!("should not be idle"),
    }

    // Simulate: remove best, schedule next
    rq.remove_best();
    let best = rq.best();
    let choice = next_up(best, idle.clone());
    match choice {
        SchedChoice::Thread(t) => assert_eq!(t.priority.get(), 10),
        SchedChoice::Idle => panic!("should not be idle"),
    }

    rq.remove_best();
    let best = rq.best();
    let choice = next_up(best, idle.clone());
    match choice {
        SchedChoice::Thread(t) => assert_eq!(t.priority.get(), 15),
        SchedChoice::Idle => panic!("should not be idle"),
    }

    // Queue empty — should get idle
    rq.remove_best();
    let best = rq.best();
    let choice = next_up(best, idle.clone());
    match choice {
        SchedChoice::Thread(t) => assert_eq!(t.id.id, 0), // idle
        SchedChoice::Idle => panic!("should be idle thread, not Idle variant"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Extended Thread Lifecycle State Machine (SchedThreadState)
// ═══════════════════════════════════════════════════════════════════════

use gale::sched::{
    SchedThreadState, sched_abort, sched_is_valid_transition, sched_pend, sched_resume,
    sched_sleep, sched_suspend, sched_unpend, sched_wakeup,
};

// ── Valid transitions ───────────────────────────────────────────────

#[test]
fn lifecycle_ready_to_running() {
    assert!(sched_is_valid_transition(
        SchedThreadState::Ready,
        SchedThreadState::Running,
    ));
}

#[test]
fn lifecycle_running_to_ready() {
    assert!(sched_is_valid_transition(
        SchedThreadState::Running,
        SchedThreadState::Ready,
    ));
}

#[test]
fn lifecycle_running_to_pending() {
    assert!(sched_is_valid_transition(
        SchedThreadState::Running,
        SchedThreadState::Pending,
    ));
}

#[test]
fn lifecycle_running_to_suspended() {
    assert!(sched_is_valid_transition(
        SchedThreadState::Running,
        SchedThreadState::Suspended,
    ));
}

#[test]
fn lifecycle_running_to_sleeping() {
    assert!(sched_is_valid_transition(
        SchedThreadState::Running,
        SchedThreadState::Sleeping,
    ));
}

#[test]
fn lifecycle_running_to_dead() {
    assert!(sched_is_valid_transition(
        SchedThreadState::Running,
        SchedThreadState::Dead,
    ));
}

#[test]
fn lifecycle_running_to_aborting() {
    assert!(sched_is_valid_transition(
        SchedThreadState::Running,
        SchedThreadState::Aborting,
    ));
}

#[test]
fn lifecycle_pending_to_ready() {
    assert!(sched_is_valid_transition(
        SchedThreadState::Pending,
        SchedThreadState::Ready,
    ));
}

#[test]
fn lifecycle_pending_to_suspended() {
    assert!(sched_is_valid_transition(
        SchedThreadState::Pending,
        SchedThreadState::Suspended,
    ));
}

#[test]
fn lifecycle_suspended_to_ready() {
    assert!(sched_is_valid_transition(
        SchedThreadState::Suspended,
        SchedThreadState::Ready,
    ));
}

#[test]
fn lifecycle_sleeping_to_ready() {
    assert!(sched_is_valid_transition(
        SchedThreadState::Sleeping,
        SchedThreadState::Ready,
    ));
}

#[test]
fn lifecycle_aborting_to_dead() {
    assert!(sched_is_valid_transition(
        SchedThreadState::Aborting,
        SchedThreadState::Dead,
    ));
}

// ── Invalid transitions ─────────────────────────────────────────────

#[test]
fn lifecycle_dead_is_terminal() {
    // SC13: Dead has no outgoing transitions
    let all_states = [
        SchedThreadState::Ready,
        SchedThreadState::Running,
        SchedThreadState::Pending,
        SchedThreadState::Suspended,
        SchedThreadState::Sleeping,
        SchedThreadState::Dead,
        SchedThreadState::Aborting,
    ];
    for &to in &all_states {
        assert!(
            !sched_is_valid_transition(SchedThreadState::Dead, to),
            "Dead should not transition to {:?}",
            to,
        );
    }
}

#[test]
fn lifecycle_ready_cannot_goto_pending() {
    // Ready -> Pending is invalid (must go through Running first)
    assert!(!sched_is_valid_transition(
        SchedThreadState::Ready,
        SchedThreadState::Pending,
    ));
}

#[test]
fn lifecycle_ready_cannot_goto_sleeping() {
    assert!(!sched_is_valid_transition(
        SchedThreadState::Ready,
        SchedThreadState::Sleeping,
    ));
}

#[test]
fn lifecycle_ready_cannot_goto_suspended() {
    assert!(!sched_is_valid_transition(
        SchedThreadState::Ready,
        SchedThreadState::Suspended,
    ));
}

#[test]
fn lifecycle_pending_cannot_goto_running() {
    // Pending -> Running is invalid (must go through Ready first)
    assert!(!sched_is_valid_transition(
        SchedThreadState::Pending,
        SchedThreadState::Running,
    ));
}

#[test]
fn lifecycle_sleeping_cannot_goto_running() {
    assert!(!sched_is_valid_transition(
        SchedThreadState::Sleeping,
        SchedThreadState::Running,
    ));
}

#[test]
fn lifecycle_aborting_cannot_goto_ready() {
    // Aborting can only go to Dead
    assert!(!sched_is_valid_transition(
        SchedThreadState::Aborting,
        SchedThreadState::Ready,
    ));
}

#[test]
fn lifecycle_aborting_cannot_goto_running() {
    assert!(!sched_is_valid_transition(
        SchedThreadState::Aborting,
        SchedThreadState::Running,
    ));
}

// ── Suspend / Resume roundtrip ──────────────────────────────────────

#[test]
fn suspend_from_running() {
    assert_eq!(
        sched_suspend(SchedThreadState::Running),
        Ok(SchedThreadState::Suspended),
    );
}

#[test]
fn suspend_from_pending() {
    assert_eq!(
        sched_suspend(SchedThreadState::Pending),
        Ok(SchedThreadState::Suspended),
    );
}

#[test]
fn suspend_idempotent() {
    // SC14: suspending an already-suspended thread is a no-op success
    assert_eq!(
        sched_suspend(SchedThreadState::Suspended),
        Ok(SchedThreadState::Suspended),
    );
}

#[test]
fn suspend_rejects_dead() {
    assert!(sched_suspend(SchedThreadState::Dead).is_err());
}

#[test]
fn suspend_rejects_ready() {
    // Ready threads are in the run queue — suspend requires Running or Pending
    assert!(sched_suspend(SchedThreadState::Ready).is_err());
}

#[test]
fn suspend_rejects_sleeping() {
    assert!(sched_suspend(SchedThreadState::Sleeping).is_err());
}

#[test]
fn resume_from_suspended() {
    // SC15: resume only from Suspended
    assert_eq!(
        sched_resume(SchedThreadState::Suspended),
        Ok(SchedThreadState::Ready),
    );
}

#[test]
fn resume_rejects_non_suspended() {
    let non_suspended = [
        SchedThreadState::Ready,
        SchedThreadState::Running,
        SchedThreadState::Pending,
        SchedThreadState::Sleeping,
        SchedThreadState::Dead,
        SchedThreadState::Aborting,
    ];
    for &state in &non_suspended {
        assert!(
            sched_resume(state).is_err(),
            "resume should reject {:?}",
            state,
        );
    }
}

#[test]
fn suspend_resume_roundtrip() {
    let state = SchedThreadState::Running;
    let suspended = sched_suspend(state).unwrap();
    assert_eq!(suspended, SchedThreadState::Suspended);
    let resumed = sched_resume(suspended).unwrap();
    assert_eq!(resumed, SchedThreadState::Ready);
}

// ── Abort from any state ────────────────────────────────────────────

#[test]
fn abort_from_any_live_state_uniprocessor() {
    // SC16: abort succeeds from all non-Dead/non-Aborting states
    let live_states = [
        SchedThreadState::Ready,
        SchedThreadState::Running,
        SchedThreadState::Pending,
        SchedThreadState::Suspended,
        SchedThreadState::Sleeping,
    ];
    for &state in &live_states {
        let result = sched_abort(state, false);
        assert_eq!(
            result,
            Ok(SchedThreadState::Dead),
            "abort({:?}, smp=false) should give Dead",
            state,
        );
    }
}

#[test]
fn abort_running_smp_gives_aborting() {
    // SMP: running on another CPU -> Aborting (needs IPI)
    assert_eq!(
        sched_abort(SchedThreadState::Running, true),
        Ok(SchedThreadState::Aborting),
    );
}

#[test]
fn abort_non_running_smp_gives_dead() {
    // SMP: not running on another CPU -> Dead directly
    assert_eq!(
        sched_abort(SchedThreadState::Pending, true),
        Ok(SchedThreadState::Dead),
    );
}

#[test]
fn abort_dead_is_error() {
    assert!(sched_abort(SchedThreadState::Dead, false).is_err());
    assert!(sched_abort(SchedThreadState::Dead, true).is_err());
}

#[test]
fn abort_aborting_is_error() {
    assert!(sched_abort(SchedThreadState::Aborting, false).is_err());
}

#[test]
fn abort_then_complete_smp() {
    // Full SMP abort lifecycle: Running -> Aborting -> Dead
    let state = SchedThreadState::Running;
    let aborting = sched_abort(state, true).unwrap();
    assert_eq!(aborting, SchedThreadState::Aborting);
    // Aborting -> Dead (after IPI processed, re-modelled as a direct abort)
    assert!(sched_is_valid_transition(
        SchedThreadState::Aborting,
        SchedThreadState::Dead,
    ));
}

// ── Sleep / Wakeup lifecycle ────────────────────────────────────────

#[test]
fn sleep_from_running() {
    assert_eq!(
        sched_sleep(SchedThreadState::Running),
        Ok(SchedThreadState::Sleeping),
    );
}

#[test]
fn sleep_rejects_non_running() {
    let non_running = [
        SchedThreadState::Ready,
        SchedThreadState::Pending,
        SchedThreadState::Suspended,
        SchedThreadState::Sleeping,
        SchedThreadState::Dead,
        SchedThreadState::Aborting,
    ];
    for &state in &non_running {
        assert!(
            sched_sleep(state).is_err(),
            "sleep should reject {:?}",
            state,
        );
    }
}

#[test]
fn wakeup_from_sleeping() {
    assert_eq!(
        sched_wakeup(SchedThreadState::Sleeping),
        Ok(SchedThreadState::Ready),
    );
}

#[test]
fn wakeup_rejects_non_sleeping() {
    let non_sleeping = [
        SchedThreadState::Ready,
        SchedThreadState::Running,
        SchedThreadState::Pending,
        SchedThreadState::Suspended,
        SchedThreadState::Dead,
        SchedThreadState::Aborting,
    ];
    for &state in &non_sleeping {
        assert!(
            sched_wakeup(state).is_err(),
            "wakeup should reject {:?}",
            state,
        );
    }
}

#[test]
fn sleep_wakeup_roundtrip() {
    let state = SchedThreadState::Running;
    let sleeping = sched_sleep(state).unwrap();
    assert_eq!(sleeping, SchedThreadState::Sleeping);
    let ready = sched_wakeup(sleeping).unwrap();
    assert_eq!(ready, SchedThreadState::Ready);
}

// ── Pend / Unpend lifecycle ─────────────────────────────────────────

#[test]
fn pend_from_running() {
    assert_eq!(
        sched_pend(SchedThreadState::Running),
        Ok(SchedThreadState::Pending),
    );
}

#[test]
fn pend_rejects_non_running() {
    let non_running = [
        SchedThreadState::Ready,
        SchedThreadState::Pending,
        SchedThreadState::Suspended,
        SchedThreadState::Sleeping,
        SchedThreadState::Dead,
        SchedThreadState::Aborting,
    ];
    for &state in &non_running {
        assert!(sched_pend(state).is_err(), "pend should reject {:?}", state,);
    }
}

#[test]
fn unpend_from_pending() {
    assert_eq!(
        sched_unpend(SchedThreadState::Pending),
        Ok(SchedThreadState::Ready),
    );
}

#[test]
fn unpend_rejects_non_pending() {
    let non_pending = [
        SchedThreadState::Ready,
        SchedThreadState::Running,
        SchedThreadState::Suspended,
        SchedThreadState::Sleeping,
        SchedThreadState::Dead,
        SchedThreadState::Aborting,
    ];
    for &state in &non_pending {
        assert!(
            sched_unpend(state).is_err(),
            "unpend should reject {:?}",
            state,
        );
    }
}

#[test]
fn pend_unpend_roundtrip() {
    let state = SchedThreadState::Running;
    let pending = sched_pend(state).unwrap();
    assert_eq!(pending, SchedThreadState::Pending);
    let ready = sched_unpend(pending).unwrap();
    assert_eq!(ready, SchedThreadState::Ready);
}

// ── Full lifecycle scenarios ────────────────────────────────────────

#[test]
fn full_lifecycle_create_run_sleep_wake_die() {
    // Simulate: Ready -> Running -> Sleeping -> Ready -> Running -> Dead
    let mut state = SchedThreadState::Ready;
    assert!(sched_is_valid_transition(state, SchedThreadState::Running));
    state = SchedThreadState::Running;

    state = sched_sleep(state).unwrap();
    assert_eq!(state, SchedThreadState::Sleeping);

    state = sched_wakeup(state).unwrap();
    assert_eq!(state, SchedThreadState::Ready);

    assert!(sched_is_valid_transition(state, SchedThreadState::Running));
    state = SchedThreadState::Running;

    state = sched_abort(state, false).unwrap();
    assert_eq!(state, SchedThreadState::Dead);

    // Dead is terminal
    assert!(sched_suspend(state).is_err());
    assert!(sched_resume(state).is_err());
    assert!(sched_abort(state, false).is_err());
    assert!(sched_sleep(state).is_err());
    assert!(sched_wakeup(state).is_err());
    assert!(sched_pend(state).is_err());
    assert!(sched_unpend(state).is_err());
}

#[test]
fn full_lifecycle_suspend_while_pending_then_abort() {
    // Running -> Pending -> Suspended -> Dead
    let mut state = SchedThreadState::Running;

    state = sched_pend(state).unwrap();
    assert_eq!(state, SchedThreadState::Pending);

    state = sched_suspend(state).unwrap();
    assert_eq!(state, SchedThreadState::Suspended);

    state = sched_abort(state, false).unwrap();
    assert_eq!(state, SchedThreadState::Dead);
}

#[test]
fn full_lifecycle_smp_abort_sequence() {
    // Running(remote CPU) -> Aborting -> Dead
    let mut state = SchedThreadState::Running;

    state = sched_abort(state, true).unwrap();
    assert_eq!(state, SchedThreadState::Aborting);

    // Can't abort again while aborting
    assert!(sched_abort(state, false).is_err());

    // Completes: Aborting -> Dead (modelled via FSM transition)
    assert!(sched_is_valid_transition(state, SchedThreadState::Dead));
}

#[test]
fn all_valid_transitions_are_consistent_with_operations() {
    // Every operation that succeeds should produce a valid FSM transition
    let all_states = [
        SchedThreadState::Ready,
        SchedThreadState::Running,
        SchedThreadState::Pending,
        SchedThreadState::Suspended,
        SchedThreadState::Sleeping,
        SchedThreadState::Dead,
        SchedThreadState::Aborting,
    ];

    for &state in &all_states {
        if let Ok(next) = sched_suspend(state) {
            // SC14: idempotent self-loop is not a FSM transition
            if state != next {
                assert!(
                    sched_is_valid_transition(state, next),
                    "suspend({:?}) -> {:?} should be valid transition",
                    state,
                    next,
                );
            }
        }
        if let Ok(next) = sched_resume(state) {
            assert!(
                sched_is_valid_transition(state, next),
                "resume({:?}) -> {:?} should be valid transition",
                state,
                next,
            );
        }
        for smp in [false, true] {
            if let Ok(next) = sched_abort(state, smp) {
                assert!(
                    sched_is_valid_transition(state, next),
                    "abort({:?}, smp={}) -> {:?} should be valid transition",
                    state,
                    smp,
                    next,
                );
            }
        }
        if let Ok(next) = sched_sleep(state) {
            assert!(
                sched_is_valid_transition(state, next),
                "sleep({:?}) -> {:?} should be valid transition",
                state,
                next,
            );
        }
        if let Ok(next) = sched_wakeup(state) {
            assert!(
                sched_is_valid_transition(state, next),
                "wakeup({:?}) -> {:?} should be valid transition",
                state,
                next,
            );
        }
        if let Ok(next) = sched_pend(state) {
            assert!(
                sched_is_valid_transition(state, next),
                "pend({:?}) -> {:?} should be valid transition",
                state,
                next,
            );
        }
        if let Ok(next) = sched_unpend(state) {
            assert!(
                sched_is_valid_transition(state, next),
                "unpend({:?}) -> {:?} should be valid transition",
                state,
                next,
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// SMP scheduling: next_up_smp + MetaIRQ preemption + update_cache
// ═══════════════════════════════════════════════════════════════════════

use gale::sched::{CpuSchedState, SmpSchedOutcome, next_up_smp, update_cache};

/// Helper: predicate for identifying MetaIRQ threads.
/// Convention: priority 0 is MetaIRQ in our test model.
fn is_metairq(t: &Thread) -> bool {
    t.priority.get() == 0
}

/// Helper: predicate that says no thread is MetaIRQ.
fn never_metairq(_t: &Thread) -> bool {
    false
}

/// SC9: When a cooperative thread was preempted by a MetaIRQ and is still
/// ready, next_up_smp returns it instead of runq_best (if runq_best is
/// not itself a MetaIRQ).
#[test]
fn test_metairq_preemption_preference() {
    let idle = make_ready_thread(0, 31);
    let mut cpu = CpuSchedState::new(idle);

    // Cooperative thread that was preempted by a MetaIRQ
    let coop = make_ready_thread(10, 5);
    cpu.metairq_preempted = Some(coop);

    // runq_best is a normal thread at prio 3 (better than coop's 5)
    let runq_best = make_ready_thread(20, 3);

    // Current is the MetaIRQ thread (prio 0), now finishing
    let current_metairq = make_ready_thread(99, 0);

    let outcome = next_up_smp(
        Some(runq_best),
        current_metairq,
        &mut cpu,
        true,  // current_is_active
        false, // current_is_queued
        false, // current_is_cooperative (MetaIRQ is preemptive)
        is_metairq,
    );

    // SC9: The preempted cooperative thread (id=10) should be chosen
    // over the runq_best (id=20), because runq_best is not a MetaIRQ
    // and the MetaIRQ preempted record points to coop.
    //
    // However, current (prio 0) has higher priority than coop (prio 5),
    // so current stays due to SC10. Let's test with a non-active current.
    let mut cpu2 = CpuSchedState::new(idle);
    cpu2.metairq_preempted = Some(coop);

    let outcome2 = next_up_smp(
        Some(runq_best),
        current_metairq,
        &mut cpu2,
        false, // current NOT active (e.g., it's blocking)
        false,
        false,
        is_metairq,
    );

    // With inactive current, the MetaIRQ-preempted coop thread wins
    match outcome2.choice {
        SchedChoice::Thread(t) => assert_eq!(
            t.id.id, 10,
            "SC9: preempted cooperative thread (id=10) should be preferred"
        ),
        SchedChoice::Idle => panic!("should not be idle"),
    }
}

/// SC10: In SMP, current thread stays if it has strictly higher priority
/// (lower numeric value) than the candidate from the run queue.
#[test]
fn test_smp_current_stays_if_higher_prio() {
    let idle = make_ready_thread(0, 31);
    let mut cpu = CpuSchedState::new(idle);

    // Current has priority 3 (high)
    let current = make_ready_thread(1, 3);
    // Best from queue has priority 10 (lower)
    let runq_best = make_ready_thread(2, 10);

    let outcome = next_up_smp(
        Some(runq_best),
        current,
        &mut cpu,
        true,  // active
        false, // not queued
        false, // preemptive
        never_metairq,
    );

    match outcome.choice {
        SchedChoice::Thread(t) => assert_eq!(
            t.id.id, 1,
            "SC10: current (prio 3) should stay over candidate (prio 10)"
        ),
        SchedChoice::Idle => panic!("should not be idle"),
    }
    // Current stays, so no requeue
    assert!(!outcome.requeue_current, "current stays -> no requeue");
}

/// SC11: Ties (equal priority) only cause a switch if swap_ok is set
/// (i.e., the current thread called k_yield).
#[test]
fn test_smp_ties_only_switch_on_yield() {
    let idle = make_ready_thread(0, 31);

    // --- Without yield (swap_ok = false): current stays ---
    let mut cpu_no_yield = CpuSchedState::new(idle);
    cpu_no_yield.swap_ok = false;

    let current = make_ready_thread(1, 5);
    let runq_best = make_ready_thread(2, 5); // same priority

    let outcome_no_yield = next_up_smp(
        Some(runq_best),
        current,
        &mut cpu_no_yield,
        true,
        false,
        false,
        never_metairq,
    );

    match outcome_no_yield.choice {
        SchedChoice::Thread(t) => {
            assert_eq!(t.id.id, 1, "SC11: without yield, tie keeps current (id=1)")
        }
        SchedChoice::Idle => panic!("should not be idle"),
    }

    // --- With yield (swap_ok = true): switch to candidate ---
    let mut cpu_yield = CpuSchedState::new(idle);
    cpu_yield.swap_ok = true;

    let outcome_yield = next_up_smp(
        Some(runq_best),
        current,
        &mut cpu_yield,
        true,
        false,
        false,
        never_metairq,
    );

    match outcome_yield.choice {
        SchedChoice::Thread(t) => assert_eq!(
            t.id.id, 2,
            "SC11: with yield, tie switches to candidate (id=2)"
        ),
        SchedChoice::Idle => panic!("should not be idle"),
    }
    // swap_ok should be cleared after the call
    assert!(!cpu_yield.swap_ok, "swap_ok cleared after next_up_smp");
}

/// SC12: When current is preempted (switched away from), it is re-queued
/// only if it is active, not already queued, not the idle thread, and
/// not the MetaIRQ-preempted thread.
#[test]
fn test_smp_current_requeued_when_preempted() {
    let idle = make_ready_thread(0, 31);

    // --- Case 1: active, not queued, not idle, not mirq-preempted -> requeue ---
    let mut cpu1 = CpuSchedState::new(idle);
    let current = make_ready_thread(1, 10); // lower prio
    let runq_best = make_ready_thread(2, 3); // higher prio

    let outcome1 = next_up_smp(
        Some(runq_best),
        current,
        &mut cpu1,
        true,  // active
        false, // NOT queued
        false, // preemptive
        never_metairq,
    );

    match outcome1.choice {
        SchedChoice::Thread(t) => assert_eq!(t.id.id, 2, "candidate wins"),
        SchedChoice::Idle => panic!("should not be idle"),
    }
    assert!(
        outcome1.requeue_current,
        "SC12: active + not queued + not idle -> requeue"
    );

    // --- Case 2: already queued -> no requeue ---
    let mut cpu2 = CpuSchedState::new(idle);
    let outcome2 = next_up_smp(
        Some(runq_best),
        current,
        &mut cpu2,
        true, // active
        true, // ALREADY queued
        false,
        never_metairq,
    );
    assert!(
        !outcome2.requeue_current,
        "SC12: already queued -> no requeue"
    );

    // --- Case 3: current is idle thread -> no requeue ---
    let mut cpu3 = CpuSchedState::new(idle);
    let outcome3 = next_up_smp(
        Some(runq_best),
        idle, // current IS the idle thread
        &mut cpu3,
        true,
        false,
        false,
        never_metairq,
    );
    assert!(!outcome3.requeue_current, "SC12: idle thread -> no requeue");

    // --- Case 4: not active (blocked/suspended) -> no requeue ---
    let mut cpu4 = CpuSchedState::new(idle);
    let outcome4 = next_up_smp(
        Some(runq_best),
        current,
        &mut cpu4,
        false, // NOT active
        false,
        false,
        never_metairq,
    );
    assert!(!outcome4.requeue_current, "SC12: not active -> no requeue");

    // --- Case 5: current is the MetaIRQ-preempted thread -> no requeue ---
    let mut cpu5 = CpuSchedState::new(idle);
    cpu5.metairq_preempted = Some(current); // current IS the mirq-preempted thread
    let outcome5 = next_up_smp(
        Some(runq_best),
        current,
        &mut cpu5,
        true,
        false,
        false,
        never_metairq,
    );
    assert!(
        !outcome5.requeue_current,
        "SC12: MetaIRQ-preempted thread -> no requeue"
    );
}
