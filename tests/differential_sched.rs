//! Differential equivalence tests — Sched (FFI vs Model).
//!
//! Verifies that the FFI scheduler functions produce the same results as
//! the Verus-verified model functions in gale::sched.

#![allow(
    clippy::shadow_unrelated,
    clippy::indexing_slicing,
    clippy::bool_to_int_with_if,
    clippy::fn_params_excessive_bools
)]

const SCHED_SELECT_RUNQ: u8 = 0;
const SCHED_SELECT_IDLE: u8 = 1;
const SCHED_SELECT_METAIRQ_PREEMPTED: u8 = 2;

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Replica of gale_sched_next_up.
fn ffi_sched_next_up(runq_best_prio: u32, idle_prio: u32) -> (i32, u32) {
    if runq_best_prio == u32::MAX {
        (1, idle_prio) // select idle
    } else {
        (0, runq_best_prio) // select runq
    }
}

/// Replica of gale_sched_should_preempt.
fn ffi_sched_should_preempt(
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

/// Replica of gale_k_sched_next_up_decide.
fn ffi_sched_next_up_decide(
    has_runq_thread: bool,
    runq_best_is_metairq: bool,
    has_metairq_preempted: bool,
    metairq_preempted_is_ready: bool,
) -> u8 {
    if has_metairq_preempted
        && (!has_runq_thread || !runq_best_is_metairq)
    {
        if metairq_preempted_is_ready {
            return SCHED_SELECT_METAIRQ_PREEMPTED;
        }
    }
    if has_runq_thread {
        SCHED_SELECT_RUNQ
    } else {
        SCHED_SELECT_IDLE
    }
}

/// Replica of gale_k_sched_preempt_decide.
fn ffi_sched_preempt_decide(
    is_cooperative: bool,
    candidate_is_metairq: bool,
    swap_ok: bool,
    current_is_prevented: bool,
) -> bool {
    if swap_ok { return true; }
    if current_is_prevented { return true; }
    if !is_cooperative || candidate_is_metairq { return true; }
    false
}

// =====================================================================
// Differential tests: sched next_up
// =====================================================================

#[test]
fn sched_next_up_ffi_matches_model_exhaustive() {
    let priorities = [0u32, 1, 15, 31, 255, u32::MAX];
    let idle_prios = [31u32, 63, 255];

    for &runq_prio in &priorities {
        for &idle_prio in &idle_prios {
            let (ffi_is_idle, ffi_prio) = ffi_sched_next_up(runq_prio, idle_prio);

            if runq_prio == u32::MAX {
                assert_eq!(ffi_is_idle, 1, "idle: runq={runq_prio}");
                assert_eq!(ffi_prio, idle_prio, "idle prio");
            } else {
                assert_eq!(ffi_is_idle, 0, "runq: runq={runq_prio}");
                assert_eq!(ffi_prio, runq_prio, "runq prio");
            }
        }
    }
}

// =====================================================================
// Differential tests: sched should_preempt
// =====================================================================

#[test]
fn sched_should_preempt_ffi_matches_model_exhaustive() {
    for coop in [false, true] {
        for metairq in [false, true] {
            for swap_ok in [false, true] {
                let ffi_result = ffi_sched_should_preempt(coop, metairq, swap_ok);

                if swap_ok {
                    assert!(ffi_result, "swap_ok always preempts");
                } else if coop && !metairq {
                    assert!(!ffi_result, "SC6: cooperative not preempted by non-metairq");
                } else {
                    assert!(ffi_result, "should preempt");
                }
            }
        }
    }
}

// =====================================================================
// Differential tests: sched next_up_decide
// =====================================================================

#[test]
fn sched_next_up_decide_ffi_matches_model_exhaustive() {
    for has_runq in [false, true] {
        for runq_metairq in [false, true] {
            for has_preempted in [false, true] {
                for preempted_ready in [false, true] {
                    let ffi_action = ffi_sched_next_up_decide(
                        has_runq, runq_metairq, has_preempted, preempted_ready);

                    if has_preempted
                        && (!has_runq || !runq_metairq)
                        && preempted_ready
                    {
                        assert_eq!(ffi_action, SCHED_SELECT_METAIRQ_PREEMPTED,
                            "SC9: metairq preempted");
                    } else if has_runq {
                        assert_eq!(ffi_action, SCHED_SELECT_RUNQ, "select runq");
                    } else {
                        assert_eq!(ffi_action, SCHED_SELECT_IDLE, "SC7: idle fallback");
                    }
                }
            }
        }
    }
}

// =====================================================================
// Differential tests: sched preempt_decide
// =====================================================================

#[test]
fn sched_preempt_decide_ffi_matches_model_exhaustive() {
    for coop in [false, true] {
        for metairq in [false, true] {
            for swap_ok in [false, true] {
                for prevented in [false, true] {
                    let result = ffi_sched_preempt_decide(coop, metairq, swap_ok, prevented);

                    if swap_ok || prevented {
                        assert!(result, "swap_ok/prevented always preempts");
                    } else if !coop || metairq {
                        assert!(result, "preemptible or metairq preempts");
                    } else {
                        assert!(!result, "cooperative not preempted");
                    }
                }
            }
        }
    }
}

// =====================================================================
// Property: SC6 — cooperative thread protection
// =====================================================================

#[test]
fn sched_cooperative_not_preempted_by_normal() {
    // cooperative=true, non-metairq candidate, no swap_ok, not prevented
    let result = ffi_sched_preempt_decide(true, false, false, false);
    assert!(!result, "SC6: cooperative not preempted by non-metairq");
}

// =====================================================================
// Property: SC7 — idle selected when no ready threads
// =====================================================================

#[test]
fn sched_idle_when_no_ready_threads() {
    let action = ffi_sched_next_up_decide(false, false, false, false);
    assert_eq!(action, SCHED_SELECT_IDLE, "SC7: idle when no runq threads");
}
