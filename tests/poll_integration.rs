//! Integration tests for the poll event state machine.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::shadow_unrelated
)]

use gale::poll::*;

// ======================================================================
// PollEvent tests
// ======================================================================

#[test]
fn event_init_not_ready() {
    let ev = PollEvent::init(TYPE_SEM_AVAILABLE, 0);
    assert_eq!(ev.state_get(), STATE_NOT_READY);
    assert!(ev.is_not_ready());
    assert!(!ev.is_ready());
    assert_eq!(ev.type_get(), TYPE_SEM_AVAILABLE);
    assert_eq!(ev.tag_get(), 0);
}

#[test]
fn event_init_with_tag() {
    let ev = PollEvent::init(TYPE_SIGNAL, 42);
    assert_eq!(ev.tag_get(), 42);
    assert_eq!(ev.type_get(), TYPE_SIGNAL);
    assert_eq!(ev.state_get(), STATE_NOT_READY);
}

#[test]
fn event_init_all_types() {
    for event_type in [
        TYPE_IGNORE,
        TYPE_SEM_AVAILABLE,
        TYPE_DATA_AVAILABLE,
        TYPE_SIGNAL,
        TYPE_MSGQ_DATA_AVAILABLE,
        TYPE_PIPE_DATA_AVAILABLE,
    ] {
        let ev = PollEvent::init(event_type, 0);
        assert_eq!(ev.state_get(), STATE_NOT_READY);
        assert_eq!(ev.type_get(), event_type);
    }
}

#[test]
fn event_set_ready() {
    let mut ev = PollEvent::init(TYPE_SEM_AVAILABLE, 0);
    ev.set_ready(STATE_SEM_AVAILABLE);
    assert!(ev.is_ready());
    assert!(!ev.is_not_ready());
    assert_eq!(ev.state_get(), STATE_SEM_AVAILABLE);
}

#[test]
fn event_set_ready_preserves_type_and_tag() {
    let mut ev = PollEvent::init(TYPE_SIGNAL, 99);
    ev.set_ready(STATE_SIGNALED);
    assert_eq!(ev.type_get(), TYPE_SIGNAL);
    assert_eq!(ev.tag_get(), 99);
}

#[test]
fn event_reset_state() {
    let mut ev = PollEvent::init(TYPE_SEM_AVAILABLE, 0);
    ev.set_ready(STATE_SEM_AVAILABLE);
    assert!(ev.is_ready());

    ev.reset_state();
    assert!(ev.is_not_ready());
    assert_eq!(ev.state_get(), STATE_NOT_READY);
    // Type and tag preserved
    assert_eq!(ev.type_get(), TYPE_SEM_AVAILABLE);
}

#[test]
fn event_reset_then_ready_again() {
    let mut ev = PollEvent::init(TYPE_SIGNAL, 0);

    // First cycle
    ev.set_ready(STATE_SIGNALED);
    assert!(ev.is_ready());
    ev.reset_state();
    assert!(ev.is_not_ready());

    // Second cycle
    ev.set_ready(STATE_SIGNALED);
    assert!(ev.is_ready());
    assert_eq!(ev.state_get(), STATE_SIGNALED);
}

#[test]
fn event_cancel() {
    let mut ev = PollEvent::init(TYPE_SEM_AVAILABLE, 0);
    ev.cancel();
    assert!(ev.is_ready());
    assert_eq!(ev.state_get(), STATE_CANCELLED);
}

#[test]
fn event_cancel_preserves_existing_state() {
    let mut ev = PollEvent::init(TYPE_SEM_AVAILABLE, 0);
    ev.set_ready(STATE_SEM_AVAILABLE);
    ev.cancel();
    // Both bits should be set (OR semantics)
    assert_eq!(ev.state_get(), STATE_SEM_AVAILABLE | STATE_CANCELLED);
}

// ======================================================================
// PollEvent condition checks
// ======================================================================

#[test]
fn check_sem_available() {
    let ev = PollEvent::init(TYPE_SEM_AVAILABLE, 0);
    assert!(ev.check_sem(1));
    assert!(ev.check_sem(100));
    assert!(!ev.check_sem(0));
}

#[test]
fn check_sem_wrong_type() {
    let ev = PollEvent::init(TYPE_SIGNAL, 0);
    assert!(!ev.check_sem(1));
    assert!(!ev.check_sem(100));
}

#[test]
fn check_signal_raised() {
    let ev = PollEvent::init(TYPE_SIGNAL, 0);
    assert!(ev.check_signal(1));
    assert!(!ev.check_signal(0));
}

#[test]
fn check_signal_wrong_type() {
    let ev = PollEvent::init(TYPE_SEM_AVAILABLE, 0);
    assert!(!ev.check_signal(1));
}

#[test]
fn check_msgq_data() {
    let ev = PollEvent::init(TYPE_MSGQ_DATA_AVAILABLE, 0);
    assert!(ev.check_msgq(1));
    assert!(ev.check_msgq(5));
    assert!(!ev.check_msgq(0));
}

#[test]
fn check_msgq_wrong_type() {
    let ev = PollEvent::init(TYPE_SEM_AVAILABLE, 0);
    assert!(!ev.check_msgq(1));
}

#[test]
fn check_data_available() {
    let ev = PollEvent::init(TYPE_DATA_AVAILABLE, 0);
    assert!(ev.check_data(true));
    assert!(!ev.check_data(false));
}

#[test]
fn check_data_wrong_type() {
    let ev = PollEvent::init(TYPE_SIGNAL, 0);
    assert!(!ev.check_data(true));
}

#[test]
fn check_pipe_data() {
    let ev = PollEvent::init(TYPE_PIPE_DATA_AVAILABLE, 0);
    assert!(ev.check_pipe(true));
    assert!(!ev.check_pipe(false));
}

#[test]
fn check_pipe_wrong_type() {
    let ev = PollEvent::init(TYPE_SEM_AVAILABLE, 0);
    assert!(!ev.check_pipe(true));
}

#[test]
fn check_ignore_type_never_ready() {
    let ev = PollEvent::init(TYPE_IGNORE, 0);
    assert!(!ev.check_sem(100));
    assert!(!ev.check_signal(1));
    assert!(!ev.check_msgq(10));
    assert!(!ev.check_data(true));
    assert!(!ev.check_pipe(true));
}

// ======================================================================
// PollSignal tests
// ======================================================================

#[test]
fn signal_init() {
    let sig = PollSignal::init();
    assert_eq!(sig.signaled, 0);
    assert_eq!(sig.result, 0);
    assert!(!sig.is_signaled());
}

#[test]
fn signal_raise() {
    let mut sig = PollSignal::init();
    sig.raise(42);
    assert!(sig.is_signaled());
    assert_eq!(sig.signaled, 1);
    assert_eq!(sig.result, 42);
}

#[test]
fn signal_raise_negative_result() {
    let mut sig = PollSignal::init();
    sig.raise(-1);
    assert!(sig.is_signaled());
    assert_eq!(sig.result, -1);
}

#[test]
fn signal_check() {
    let mut sig = PollSignal::init();
    let (s, r) = sig.check();
    assert_eq!(s, 0);
    assert_eq!(r, 0);

    sig.raise(99);
    let (s, r) = sig.check();
    assert_eq!(s, 1);
    assert_eq!(r, 99);
}

#[test]
fn signal_reset() {
    let mut sig = PollSignal::init();
    sig.raise(42);
    assert!(sig.is_signaled());

    sig.reset();
    assert!(!sig.is_signaled());
    assert_eq!(sig.signaled, 0);
    // Zephyr does NOT clear result on reset
    assert_eq!(sig.result, 42);
}

#[test]
fn signal_raise_reset_raise() {
    let mut sig = PollSignal::init();
    sig.raise(10);
    assert_eq!(sig.result, 10);

    sig.reset();
    assert!(!sig.is_signaled());

    sig.raise(20);
    assert!(sig.is_signaled());
    assert_eq!(sig.result, 20);
}

#[test]
fn signal_double_raise() {
    let mut sig = PollSignal::init();
    sig.raise(1);
    sig.raise(2);
    assert!(sig.is_signaled());
    assert_eq!(sig.result, 2); // second raise overwrites result
}

#[test]
fn signal_clone_and_equality() {
    let mut sig = PollSignal::init();
    sig.raise(42);
    let sig2 = sig.clone();
    assert_eq!(sig, sig2);

    sig.reset();
    assert_ne!(sig, sig2);
}

// ======================================================================
// PollEvents (array) tests
// ======================================================================

#[test]
fn events_new_empty() {
    let events = PollEvents::new();
    assert_eq!(events.len(), 0);
    assert!(!events.any_ready());
    assert_eq!(events.count_ready(), 0);
}

#[test]
fn events_add_single() {
    let mut events = PollEvents::new();
    let ev = PollEvent::init(TYPE_SEM_AVAILABLE, 0);
    assert!(events.add(ev));
    assert_eq!(events.len(), 1);
}

#[test]
fn events_add_multiple() {
    let mut events = PollEvents::new();
    for i in 0..5 {
        let ev = PollEvent::init(TYPE_SEM_AVAILABLE, i);
        assert!(events.add(ev));
    }
    assert_eq!(events.len(), 5);
}

#[test]
fn events_add_full() {
    let mut events = PollEvents::new();
    for i in 0..16 {
        let ev = PollEvent::init(TYPE_SEM_AVAILABLE, i);
        assert!(events.add(ev));
    }
    assert_eq!(events.len(), 16);
    // 17th should fail
    let ev = PollEvent::init(TYPE_SEM_AVAILABLE, 99);
    assert!(!events.add(ev));
    assert_eq!(events.len(), 16);
}

#[test]
fn events_any_ready_none() {
    let mut events = PollEvents::new();
    events.add(PollEvent::init(TYPE_SEM_AVAILABLE, 0));
    events.add(PollEvent::init(TYPE_SIGNAL, 1));
    assert!(!events.any_ready());
}

#[test]
fn events_any_ready_one() {
    let mut events = PollEvents::new();
    events.add(PollEvent::init(TYPE_SEM_AVAILABLE, 0));
    events.add(PollEvent::init(TYPE_SIGNAL, 1));

    // Set second event ready
    events.events[1].set_ready(STATE_SIGNALED);
    assert!(events.any_ready());
    assert_eq!(events.count_ready(), 1);
}

#[test]
fn events_any_ready_all() {
    let mut events = PollEvents::new();
    events.add(PollEvent::init(TYPE_SEM_AVAILABLE, 0));
    events.add(PollEvent::init(TYPE_SIGNAL, 1));
    events.add(PollEvent::init(TYPE_MSGQ_DATA_AVAILABLE, 2));

    events.events[0].set_ready(STATE_SEM_AVAILABLE);
    events.events[1].set_ready(STATE_SIGNALED);
    events.events[2].set_ready(STATE_MSGQ_DATA_AVAILABLE);

    assert!(events.any_ready());
    assert_eq!(events.count_ready(), 3);
}

#[test]
fn events_reset_all_states() {
    let mut events = PollEvents::new();
    events.add(PollEvent::init(TYPE_SEM_AVAILABLE, 0));
    events.add(PollEvent::init(TYPE_SIGNAL, 1));

    events.events[0].set_ready(STATE_SEM_AVAILABLE);
    events.events[1].set_ready(STATE_SIGNALED);
    assert!(events.any_ready());

    events.reset_all_states();
    assert!(!events.any_ready());
    assert_eq!(events.count_ready(), 0);
    // Types preserved
    assert_eq!(events.events[0].type_get(), TYPE_SEM_AVAILABLE);
    assert_eq!(events.events[1].type_get(), TYPE_SIGNAL);
}

// ======================================================================
// End-to-end poll simulation
// ======================================================================

#[test]
fn poll_simulation_sem_ready() {
    // Simulate: poll on semaphore, semaphore becomes available
    let mut events = PollEvents::new();
    events.add(PollEvent::init(TYPE_SEM_AVAILABLE, 0));

    // PL6: reset before poll
    events.reset_all_states();

    // Check condition: sem count = 3
    let sem_count = 3u32;
    if events.events[0].check_sem(sem_count) {
        events.events[0].set_ready(STATE_SEM_AVAILABLE);
    }

    // PL5: poll returns because an event is ready
    assert!(events.any_ready());
    assert_eq!(events.events[0].state_get(), STATE_SEM_AVAILABLE);
}

#[test]
fn poll_simulation_signal_ready() {
    let mut events = PollEvents::new();
    events.add(PollEvent::init(TYPE_SIGNAL, 0));

    events.reset_all_states();

    // Signal is raised
    let mut sig = PollSignal::init();
    sig.raise(7);

    if events.events[0].check_signal(sig.signaled) {
        events.events[0].set_ready(STATE_SIGNALED);
    }

    assert!(events.any_ready());
    assert_eq!(events.events[0].state_get(), STATE_SIGNALED);
}

#[test]
fn poll_simulation_multiple_one_ready() {
    let mut events = PollEvents::new();
    events.add(PollEvent::init(TYPE_SEM_AVAILABLE, 0));
    events.add(PollEvent::init(TYPE_SIGNAL, 1));
    events.add(PollEvent::init(TYPE_MSGQ_DATA_AVAILABLE, 2));

    events.reset_all_states();

    // Only msgq has data
    let sem_count = 0u32;
    let sig = PollSignal::init();
    let msgq_used = 5u32;

    if events.events[0].check_sem(sem_count) {
        events.events[0].set_ready(STATE_SEM_AVAILABLE);
    }
    if events.events[1].check_signal(sig.signaled) {
        events.events[1].set_ready(STATE_SIGNALED);
    }
    if events.events[2].check_msgq(msgq_used) {
        events.events[2].set_ready(STATE_MSGQ_DATA_AVAILABLE);
    }

    assert!(events.any_ready());
    assert_eq!(events.count_ready(), 1);
    assert!(events.events[0].is_not_ready());
    assert!(events.events[1].is_not_ready());
    assert!(events.events[2].is_ready());
}

#[test]
fn poll_simulation_none_ready() {
    let mut events = PollEvents::new();
    events.add(PollEvent::init(TYPE_SEM_AVAILABLE, 0));
    events.add(PollEvent::init(TYPE_SIGNAL, 1));

    events.reset_all_states();

    // Nothing is available
    if events.events[0].check_sem(0) {
        events.events[0].set_ready(STATE_SEM_AVAILABLE);
    }
    let sig = PollSignal::init();
    if events.events[1].check_signal(sig.signaled) {
        events.events[1].set_ready(STATE_SIGNALED);
    }

    // Would block / return -EAGAIN
    assert!(!events.any_ready());
}

#[test]
fn poll_simulation_repeated_cycles() {
    let mut events = PollEvents::new();
    events.add(PollEvent::init(TYPE_SEM_AVAILABLE, 0));

    // Cycle 1: not ready
    events.reset_all_states();
    assert!(!events.any_ready());

    // Cycle 2: ready
    events.reset_all_states();
    if events.events[0].check_sem(1) {
        events.events[0].set_ready(STATE_SEM_AVAILABLE);
    }
    assert!(events.any_ready());

    // Cycle 3: not ready again
    events.reset_all_states();
    assert!(!events.any_ready());
}

#[test]
fn event_clone_and_equality() {
    let mut ev = PollEvent::init(TYPE_SIGNAL, 5);
    let ev2 = ev.clone();
    assert_eq!(ev, ev2);

    ev.set_ready(STATE_SIGNALED);
    assert_ne!(ev, ev2);
}
