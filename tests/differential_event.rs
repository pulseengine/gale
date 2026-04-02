//! Differential equivalence tests — Event (FFI vs Model).
//!
//! Verifies that the FFI event functions produce the same results as
//! the Verus-verified model functions in gale::event.

#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use gale::event::{self, Event, WaitDecision, WAIT_ALL, WAIT_ANY};

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Replica of gale_event_post.
fn ffi_event_post(events: u32, new_events: u32) -> u32 {
    events | new_events
}

/// Replica of gale_event_set.
/// Returns the old events value; the caller uses new_events directly.
fn ffi_event_set(current: u32) -> u32 {
    current // returns old value
}

/// Replica of gale_event_clear.
fn ffi_event_clear(events: u32, clear_bits: u32) -> u32 {
    events & !clear_bits
}

/// Replica of gale_event_set_masked.
fn ffi_event_set_masked(events: u32, new_bits: u32, mask: u32) -> u32 {
    (events & !mask) | (new_bits & mask)
}

/// Replica of gale_event_wait_check_any.
fn ffi_event_wait_check_any(events: u32, desired: u32) -> bool {
    (events & desired) != 0
}

/// Replica of gale_event_wait_check_all.
fn ffi_event_wait_check_all(events: u32, desired: u32) -> bool {
    (events & desired) == desired
}

/// Replica of gale_k_event_post_decide.
fn ffi_event_post_decide(current_events: u32, new_events: u32, mask: u32) -> u32 {
    event::post_decide(current_events, new_events, mask)
}

/// Replica of gale_k_event_wait_decide.
/// Returns (ret, matched_events, action).
fn ffi_event_wait_decide(
    current_events: u32,
    desired: u32,
    wait_type: u8,
    is_no_wait: bool,
) -> (i32, u32, u8) {
    let r = event::wait_decide(current_events, desired, wait_type, is_no_wait);
    match r.decision {
        WaitDecision::Matched => (0, r.matched_events, 0),
        WaitDecision::Pend => (0, 0, 1),
        WaitDecision::Timeout => (0, 0, 2),
    }
}

// =====================================================================
// Differential tests: event_post
// =====================================================================

#[test]
fn event_post_ffi_matches_model_exhaustive() {
    for events in 0u32..=0xFF {
        for new_events in 0u32..=0xFF {
            let ffi_result = ffi_event_post(events, new_events);

            let mut e = Event::init();
            e.events = events;
            e.post(new_events);

            assert_eq!(ffi_result, e.events_get(),
                "post diverged: events=0x{events:X}, new=0x{new_events:X}");
        }
    }
}

// =====================================================================
// Differential tests: event_set
// =====================================================================

#[test]
fn event_set_ffi_matches_model() {
    for current in [0u32, 0xFF, 0xDEAD, u32::MAX] {
        for new_events in [0u32, 0x42, 0xFF00, u32::MAX] {
            let ffi_old = ffi_event_set(current);

            let mut e = Event::init();
            e.events = current;
            let model_old = e.set(new_events);

            assert_eq!(ffi_old, model_old,
                "set old_events diverged: current=0x{current:X}, new=0x{new_events:X}");
            assert_eq!(e.events_get(), new_events,
                "set result diverged: current=0x{current:X}, new=0x{new_events:X}");
        }
    }
}

// =====================================================================
// Differential tests: event_clear
// =====================================================================

#[test]
fn event_clear_ffi_matches_model_exhaustive() {
    for events in 0u32..=0xFF {
        for clear_bits in 0u32..=0xFF {
            let ffi_result = ffi_event_clear(events, clear_bits);

            let mut e = Event::init();
            e.events = events;
            let model_result = e.clear(clear_bits);

            assert_eq!(ffi_result, model_result,
                "clear diverged: events=0x{events:X}, clear=0x{clear_bits:X}");
            assert_eq!(ffi_result, e.events_get());
        }
    }
}

// =====================================================================
// Differential tests: event_set_masked
// =====================================================================

#[test]
fn event_set_masked_ffi_matches_model_exhaustive() {
    for events in (0u32..=0xF).map(|x| x << 4 | x) {
        for new_bits in (0u32..=0xF).map(|x| x << 4 | x) {
            for mask in (0u32..=0xF).map(|x| x << 4 | x) {
                let ffi_result = ffi_event_set_masked(events, new_bits, mask);

                let mut e = Event::init();
                e.events = events;
                e.set_masked(new_bits, mask);

                assert_eq!(ffi_result, e.events_get(),
                    "set_masked diverged: events=0x{events:X}, new=0x{new_bits:X}, mask=0x{mask:X}");
            }
        }
    }
}

// =====================================================================
// Differential tests: event_wait_check_any / _all
// =====================================================================

#[test]
fn event_wait_check_ffi_matches_model_exhaustive() {
    for events in 0u32..=0xFF {
        for desired in 0u32..=0xFF {
            let ffi_any = ffi_event_wait_check_any(events, desired);
            let ffi_all = ffi_event_wait_check_all(events, desired);

            let e = Event { events };
            let model_any = e.wait_check_any(desired);
            let model_all = e.wait_check_all(desired);

            assert_eq!(ffi_any, model_any,
                "wait_check_any diverged: events=0x{events:X}, desired=0x{desired:X}");
            assert_eq!(ffi_all, model_all,
                "wait_check_all diverged: events=0x{events:X}, desired=0x{desired:X}");
        }
    }
}

// =====================================================================
// Differential tests: event_post_decide (EV4)
// =====================================================================

#[test]
fn event_post_decide_ffi_matches_model_exhaustive() {
    for current in 0u32..=0xF {
        for new_events in 0u32..=0xF {
            for mask in 0u32..=0xF {
                let ffi_result = ffi_event_post_decide(current, new_events, mask);

                // Model: manual computation of (current & !mask) | (new & mask)
                let expected = (current & !mask) | (new_events & mask);

                assert_eq!(ffi_result, expected,
                    "post_decide diverged: current=0x{current:X}, new=0x{new_events:X}, mask=0x{mask:X}");

                // Also verify via Event::set_masked
                let mut e = Event { events: current };
                e.set_masked(new_events, mask);
                assert_eq!(ffi_result, e.events_get(),
                    "post_decide vs set_masked diverged");
            }
        }
    }
}

// =====================================================================
// Differential tests: event_wait_decide
// =====================================================================

#[test]
fn event_wait_decide_ffi_matches_model_exhaustive() {
    for current in 0u32..=0xF {
        for desired in 0u32..=0xF {
            for wait_type in [WAIT_ANY, WAIT_ALL] {
                for is_no_wait in [false, true] {
                    let (ffi_ret, ffi_matched, ffi_action) =
                        ffi_event_wait_decide(current, desired, wait_type, is_no_wait);

                    // Expected behavior
                    let condition_met = if wait_type == WAIT_ALL {
                        (current & desired) == desired
                    } else {
                        (current & desired) != 0
                    };

                    if condition_met {
                        assert_eq!(ffi_action, 0,
                            "should be MATCHED: current=0x{current:X}, desired=0x{desired:X}");
                        assert_eq!(ffi_matched, current & desired,
                            "matched bits wrong: current=0x{current:X}, desired=0x{desired:X}");
                        assert_eq!(ffi_ret, 0);
                    } else if is_no_wait {
                        assert_eq!(ffi_action, 2,
                            "should be TIMEOUT: current=0x{current:X}, desired=0x{desired:X}");
                        assert_eq!(ffi_matched, 0);
                    } else {
                        assert_eq!(ffi_action, 1,
                            "should be PEND: current=0x{current:X}, desired=0x{desired:X}");
                        assert_eq!(ffi_matched, 0);
                    }
                }
            }
        }
    }
}

// =====================================================================
// Property: EV1 — post is monotonic (never clears bits)
// =====================================================================

#[test]
fn event_post_is_monotonic() {
    let mut rng: u32 = 0xCAFE_BABE;
    let mut e = Event::init();
    let mut ffi_events = 0u32;

    for _ in 0..500 {
        rng = rng.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        let new_bits = rng >> 16;
        let old_model = e.events_get();
        let old_ffi = ffi_events;

        e.post(new_bits);
        ffi_events = ffi_event_post(ffi_events, new_bits);

        // EV1: old bits preserved
        assert_eq!(e.events_get() & old_model, old_model, "EV1: model lost bits");
        assert_eq!(ffi_events & old_ffi, old_ffi, "EV1: FFI lost bits");
        // FFI matches model
        assert_eq!(ffi_events, e.events_get(), "FFI/model diverged");
    }
}

// =====================================================================
// Random operations: FFI matches model through a sequence
// =====================================================================

#[test]
fn event_random_ops_ffi_matches_model() {
    let mut model = Event::init();
    let mut ffi_events = 0u32;

    let mut rng: u32 = 0xDEAD_C0DE;
    for _ in 0..1000 {
        rng = rng.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        let bits = rng >> 16;

        match rng % 4 {
            0 => {
                // Post
                model.post(bits);
                ffi_events = ffi_event_post(ffi_events, bits);
            }
            1 => {
                // Clear
                model.clear(bits);
                ffi_events = ffi_event_clear(ffi_events, bits);
            }
            2 => {
                // Set
                model.set(bits);
                ffi_events = bits;
            }
            _ => {
                // Set masked
                let mask = rng >> 8;
                model.set_masked(bits, mask);
                ffi_events = ffi_event_set_masked(ffi_events, bits, mask);
            }
        }
        assert_eq!(ffi_events, model.events_get(),
            "FFI/model event diverged at rng={rng}");
    }
}
