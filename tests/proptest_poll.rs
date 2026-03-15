//! Property-based tests for the poll event state machine.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing
)]

use gale::poll::*;
use proptest::prelude::*;

/// Strategy to generate valid poll event types.
fn valid_event_type() -> impl Strategy<Value = u32> {
    prop_oneof![
        Just(TYPE_IGNORE),
        Just(TYPE_SEM_AVAILABLE),
        Just(TYPE_DATA_AVAILABLE),
        Just(TYPE_SIGNAL),
        Just(TYPE_MSGQ_DATA_AVAILABLE),
        Just(TYPE_PIPE_DATA_AVAILABLE),
    ]
}

/// Strategy to generate valid poll event states.
fn valid_event_state() -> impl Strategy<Value = u32> {
    prop_oneof![
        Just(STATE_NOT_READY),
        Just(STATE_SEM_AVAILABLE),
        Just(STATE_DATA_AVAILABLE),
        Just(STATE_SIGNALED),
        Just(STATE_MSGQ_DATA_AVAILABLE),
        Just(STATE_PIPE_DATA_AVAILABLE),
        Just(STATE_CANCELLED),
    ]
}

proptest! {
    /// PL1: init always produces NOT_READY state.
    #[test]
    fn init_always_not_ready(
        event_type in valid_event_type(),
        tag in any::<u32>()
    ) {
        let ev = PollEvent::init(event_type, tag);
        prop_assert_eq!(ev.state_get(), STATE_NOT_READY);
        prop_assert!(ev.is_not_ready());
        prop_assert!(!ev.is_ready());
        prop_assert_eq!(ev.type_get(), event_type);
        prop_assert_eq!(ev.tag_get(), tag);
    }

    /// PL6: reset_state always returns to NOT_READY.
    #[test]
    fn reset_always_not_ready(
        event_type in valid_event_type(),
        state in valid_event_state(),
        tag in any::<u32>()
    ) {
        let mut ev = PollEvent::init(event_type, tag);
        // Artificially set state
        if state != STATE_NOT_READY {
            ev.set_ready(state);
        }
        ev.reset_state();
        prop_assert_eq!(ev.state_get(), STATE_NOT_READY);
        prop_assert!(ev.is_not_ready());
        // Type and tag preserved
        prop_assert_eq!(ev.type_get(), event_type);
        prop_assert_eq!(ev.tag_get(), tag);
    }

    /// PL2: set_ready ORs state bits.
    #[test]
    fn set_ready_ors_state(
        event_type in valid_event_type(),
        state1 in valid_event_state().prop_filter("non-zero", |s| *s != STATE_NOT_READY),
        state2 in valid_event_state().prop_filter("non-zero", |s| *s != STATE_NOT_READY),
    ) {
        let mut ev = PollEvent::init(event_type, 0);
        ev.set_ready(state1);
        let after_first = ev.state_get();
        prop_assert_eq!(after_first, state1);

        ev.set_ready(state2);
        prop_assert_eq!(ev.state_get(), state1 | state2);
    }

    /// PL3: check_sem is true iff type==SEM_AVAILABLE and count > 0.
    #[test]
    fn check_sem_correct(
        event_type in valid_event_type(),
        sem_count in any::<u32>()
    ) {
        let ev = PollEvent::init(event_type, 0);
        let expected = event_type == TYPE_SEM_AVAILABLE && sem_count > 0;
        prop_assert_eq!(ev.check_sem(sem_count), expected);
    }

    /// PL4: check_signal is true iff type==SIGNAL and signaled != 0.
    #[test]
    fn check_signal_correct(
        event_type in valid_event_type(),
        signaled in any::<u32>()
    ) {
        let ev = PollEvent::init(event_type, 0);
        let expected = event_type == TYPE_SIGNAL && signaled != 0;
        prop_assert_eq!(ev.check_signal(signaled), expected);
    }

    /// check_msgq correct: true iff type==MSGQ and used > 0.
    #[test]
    fn check_msgq_correct(
        event_type in valid_event_type(),
        used_msgs in any::<u32>()
    ) {
        let ev = PollEvent::init(event_type, 0);
        let expected = event_type == TYPE_MSGQ_DATA_AVAILABLE && used_msgs > 0;
        prop_assert_eq!(ev.check_msgq(used_msgs), expected);
    }

    /// check_data correct: true iff type==DATA_AVAILABLE and not empty.
    #[test]
    fn check_data_correct(
        event_type in valid_event_type(),
        not_empty in any::<bool>()
    ) {
        let ev = PollEvent::init(event_type, 0);
        let expected = event_type == TYPE_DATA_AVAILABLE && not_empty;
        prop_assert_eq!(ev.check_data(not_empty), expected);
    }

    /// check_pipe correct: true iff type==PIPE and not empty.
    #[test]
    fn check_pipe_correct(
        event_type in valid_event_type(),
        not_empty in any::<bool>()
    ) {
        let ev = PollEvent::init(event_type, 0);
        let expected = event_type == TYPE_PIPE_DATA_AVAILABLE && not_empty;
        prop_assert_eq!(ev.check_pipe(not_empty), expected);
    }

    /// PL7: signal raise sets signaled=1 and result.
    #[test]
    fn signal_raise_correct(result_val in any::<i32>()) {
        let mut sig = PollSignal::init();
        sig.raise(result_val);
        prop_assert_eq!(sig.signaled, 1);
        prop_assert_eq!(sig.result, result_val);
        prop_assert!(sig.is_signaled());
    }

    /// PL8: signal reset clears signaled but preserves result.
    #[test]
    fn signal_reset_preserves_result(result_val in any::<i32>()) {
        let mut sig = PollSignal::init();
        sig.raise(result_val);
        sig.reset();
        prop_assert_eq!(sig.signaled, 0);
        prop_assert!(!sig.is_signaled());
        prop_assert_eq!(sig.result, result_val);
    }

    /// Signal check returns current state.
    #[test]
    fn signal_check_returns_state(
        result_val in any::<i32>(),
        do_raise in any::<bool>()
    ) {
        let mut sig = PollSignal::init();
        if do_raise {
            sig.raise(result_val);
        }
        let (s, r) = sig.check();
        prop_assert_eq!(s, sig.signaled);
        prop_assert_eq!(r, sig.result);
    }

    /// PL7+PL8 roundtrip: raise then reset then raise.
    #[test]
    fn signal_raise_reset_raise_roundtrip(
        val1 in any::<i32>(),
        val2 in any::<i32>()
    ) {
        let mut sig = PollSignal::init();
        sig.raise(val1);
        prop_assert!(sig.is_signaled());

        sig.reset();
        prop_assert!(!sig.is_signaled());
        // Result is preserved from last raise
        prop_assert_eq!(sig.result, val1);

        sig.raise(val2);
        prop_assert!(sig.is_signaled());
        prop_assert_eq!(sig.result, val2);
    }

    /// PollEvents: add up to MAX then overflow returns false.
    #[test]
    fn events_add_respects_capacity(
        n in 0u32..20
    ) {
        let mut events = PollEvents::new();
        let mut succeeded = 0u32;
        for _ in 0..n {
            let ev = PollEvent::init(TYPE_SEM_AVAILABLE, 0);
            if events.add(ev) {
                succeeded += 1;
            }
        }
        let expected = if n <= MAX_POLL_EVENTS { n } else { MAX_POLL_EVENTS };
        prop_assert_eq!(succeeded, expected);
        prop_assert_eq!(events.len(), expected);
    }

    /// PL5+PL6: reset then check = none ready; set one ready, any_ready = true.
    #[test]
    fn events_reset_then_set_one_ready(
        n in 1u32..8,
        ready_idx_raw in any::<u32>()
    ) {
        let mut events = PollEvents::new();
        for i in 0..n {
            events.add(PollEvent::init(TYPE_SEM_AVAILABLE, i));
        }
        events.reset_all_states();
        prop_assert!(!events.any_ready());

        let ready_idx = (ready_idx_raw % n) as usize;
        events.events[ready_idx].set_ready(STATE_SEM_AVAILABLE);

        prop_assert!(events.any_ready());
        prop_assert_eq!(events.count_ready(), 1);
    }
}
