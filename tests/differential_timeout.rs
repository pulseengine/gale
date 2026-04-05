//! Differential equivalence tests — Timeout (FFI vs Model).
//!
//! Verifies that the FFI timeout functions produce the same results as
//! the Verus-verified model functions in gale::timeout.

#![allow(
    clippy::shadow_unrelated,
    clippy::indexing_slicing,
    clippy::bool_to_int_with_if,
    clippy::absurd_extreme_comparisons,
    clippy::branches_sharing_code
)]

use gale::error::*;
use gale::timeout::Timeout;

const K_FOREVER_TICKS: u64 = u64::MAX;

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Replica of gale_timeout_add_decide.
fn ffi_timeout_add_decide(current_tick: u64, duration: u64) -> (i32, u64) {
    if current_tick >= K_FOREVER_TICKS {
        return (EINVAL, 0);
    }
    #[allow(clippy::arithmetic_side_effects)]
    if duration >= K_FOREVER_TICKS - current_tick {
        return (EINVAL, 0);
    }
    #[allow(clippy::arithmetic_side_effects)]
    let dl = current_tick + duration;
    (OK, dl)
}

/// Replica of gale_timeout_abort_decide.
fn ffi_timeout_abort_decide(is_linked: bool) -> (i32, u8) {
    if is_linked {
        (OK, 0) // REMOVE
    } else {
        (EINVAL, 1) // NOOP
    }
}

/// Replica of gale_timeout_announce_decide.
fn ffi_timeout_announce_decide(
    current_tick: u64,
    ticks: u64,
    deadline: u64,
    active: bool,
) -> (i32, u64, bool) {
    #[allow(clippy::arithmetic_side_effects)]
    if ticks >= K_FOREVER_TICKS - current_tick {
        return (EINVAL, 0, false);
    }
    #[allow(clippy::arithmetic_side_effects)]
    let advanced = current_tick + ticks;

    let fired = active
        && deadline != K_FOREVER_TICKS
        && deadline <= advanced;

    (OK, advanced, fired)
}

// =====================================================================
// Differential tests: timeout add_decide
// =====================================================================

#[test]
fn timeout_add_decide_ffi_matches_model_exhaustive() {
    let ticks = [0u64, 1, 10, 100, 1000, u64::MAX / 2, u64::MAX - 2, u64::MAX - 1];
    let durations = [0u64, 1, 10, 100, u64::MAX / 2, u64::MAX - 2, u64::MAX - 1, u64::MAX];

    for &current_tick in &ticks {
        for &duration in &durations {
            let (ffi_ret, ffi_dl) = ffi_timeout_add_decide(current_tick, duration);

            if current_tick < K_FOREVER_TICKS {
                let mut t = Timeout::init(current_tick);
                let model_result = t.add(duration);

                match model_result {
                    Ok(dl) => {
                        assert_eq!(ffi_ret, OK,
                            "add ret: tick={current_tick}, dur={duration}");
                        assert_eq!(ffi_dl, dl,
                            "add deadline: tick={current_tick}, dur={duration}");
                    }
                    Err(_e) => {
                        assert_eq!(ffi_ret, EINVAL,
                            "add err: tick={current_tick}, dur={duration}");
                    }
                }
            }
        }
    }
}

// =====================================================================
// Differential tests: timeout abort_decide
// =====================================================================

#[test]
fn timeout_abort_decide_ffi_matches_model_exhaustive() {
    for is_linked in [false, true] {
        let (ffi_ret, ffi_action) = ffi_timeout_abort_decide(is_linked);

        if is_linked {
            assert_eq!(ffi_ret, OK);
            assert_eq!(ffi_action, 0); // REMOVE
        } else {
            assert_eq!(ffi_ret, EINVAL);
            assert_eq!(ffi_action, 1); // NOOP
        }

        // Cross-check with model: abort on active vs inactive
        let mut t = Timeout::init(100);
        if is_linked {
            let _ = t.add(50);
            let model_ret = t.abort();
            assert_eq!(ffi_ret, model_ret);
        } else {
            let model_ret = t.abort();
            assert_eq!(ffi_ret, model_ret);
        }
    }
}

// =====================================================================
// Differential tests: timeout announce_decide
// =====================================================================

#[test]
fn timeout_announce_decide_ffi_matches_model() {
    let test_cases: Vec<(u64, u64, u64, bool)> = vec![
        // (current_tick, ticks_to_advance, deadline, active)
        (0, 10, 5, true),    // fires: deadline 5 <= new_tick 10
        (0, 10, 15, true),   // doesn't fire: deadline 15 > new_tick 10
        (0, 10, 10, true),   // fires: deadline 10 <= new_tick 10
        (0, 10, 5, false),   // inactive: doesn't fire
        (0, 10, K_FOREVER_TICKS, true), // forever: doesn't fire
        (100, 50, 120, true),   // fires
        (100, 50, 200, true),   // doesn't fire
        (0, 0, 0, true),       // fires: deadline 0 <= new_tick 0
    ];

    for (current_tick, ticks, deadline, active) in test_cases {
        let (ffi_ret, ffi_new_tick, ffi_fired) =
            ffi_timeout_announce_decide(current_tick, ticks, deadline, active);

        let mut t = Timeout {
            deadline,
            active,
            current_tick,
        };
        let model_result = t.announce(ticks);

        match model_result {
            Ok(fired) => {
                assert_eq!(ffi_ret, OK,
                    "announce ret: tick={current_tick}, ticks={ticks}");
                #[allow(clippy::arithmetic_side_effects)]
                let expected_tick = current_tick + ticks;
                assert_eq!(ffi_new_tick, expected_tick,
                    "announce new_tick: tick={current_tick}, ticks={ticks}");
                assert_eq!(ffi_fired, fired,
                    "announce fired: tick={current_tick}, ticks={ticks}, dl={deadline}, active={active}");
            }
            Err(_) => {
                assert_eq!(ffi_ret, EINVAL,
                    "announce err: tick={current_tick}, ticks={ticks}");
            }
        }
    }
}

// =====================================================================
// Property: TO2 — add sets deadline = current_tick + duration
// =====================================================================

#[test]
fn timeout_add_deadline_correct() {
    for tick in [0u64, 1, 100, 1000, u64::MAX / 4] {
        for dur in [0u64, 1, 100, u64::MAX / 4] {
            let (ret, dl) = ffi_timeout_add_decide(tick, dur);
            if ret == OK {
                #[allow(clippy::arithmetic_side_effects)]
                let expected = tick + dur;
                assert_eq!(dl, expected, "TO2 violated: tick={tick}, dur={dur}");
            }
        }
    }
}

// =====================================================================
// Property: TO7 — K_FOREVER never fires
// =====================================================================

#[test]
fn timeout_forever_never_fires() {
    for ticks_advance in [1u64, 100, 1_000_000, u64::MAX / 2] {
        let (ret, _, fired) =
            ffi_timeout_announce_decide(0, ticks_advance, K_FOREVER_TICKS, true);
        if ret == OK {
            assert!(!fired, "TO7 violated: K_FOREVER should never fire");
        }
    }
}
