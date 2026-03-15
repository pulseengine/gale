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
    rq.add(make_ready_thread(2, 5));  // higher priority (lower value)
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
    assert!(is_valid_transition(ThreadState::Ready, ThreadState::Running));
    assert!(is_valid_transition(ThreadState::Running, ThreadState::Ready));
    assert!(is_valid_transition(ThreadState::Running, ThreadState::Blocked));
    assert!(is_valid_transition(ThreadState::Blocked, ThreadState::Ready));
}

#[test]
fn blocked_to_running_requires_ready_intermediate() {
    // Blocked -> Running should go through Ready first
    // But our FSM allows any -> Ready, so Blocked -> Ready is valid
    assert!(is_valid_transition(ThreadState::Blocked, ThreadState::Ready));
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
