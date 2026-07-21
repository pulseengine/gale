//! gust-timer-probe — end-to-end tickless-sleep demonstrator for the
//! verified gust:os executor (VER-OS-TIMER-001). The scheduler core
//! (`gale::executor::Tasks`, `src/executor.rs`) is ALREADY Verus-proven
//! (`inv()` preserved by every mutator, no-lost-wakeups, tickless
//! `next_deadline`/`expire`/`slept_status` contracts) — this probe does NOT
//! re-prove those. It drives the REAL executor DIRECTLY (native, like
//! `gust_hm_probe` drives `gale::health_monitor`) on qemu and reads back
//! actual `Tasks` state/return values after each step — no shadow/parallel
//! bookkeeping — to demonstrate the tickless sleep mechanism end to end:
//! one task sleeps for `ONE_SEC`, the outer layer arms exactly ONE alarm at
//! the deadline (no periodic tick), the task is provably not readied before
//! that instant, and is readied and reports `elapsed` exactly at it.
//!
//! Non-vacuity: an out-of-range handle reports the sentinel `0xFFFF_FFFF`
//! (not readied by nothing), and a fresh task's default deadline
//! (`u64::MAX`, `Tasks::new`'s initializer) reports `pending` — so the
//! `elapsed`/`invalid` results above are genuine, not the function's only
//! reachable output.
//!
//! No fake passes: every assertion reads `tasks.state`/`tasks.ready` or a
//! `Tasks` method's real return value — no recomputed shadow deadline/status.
//! Exit codes: semihosting EXIT_SUCCESS / EXIT_FAILURE, no silent
//! fall-through OK path.
#![no_std]
#![no_main]

use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use gale::executor::{TaskState, Tasks, MAX_TASKS};
use panic_halt as _;

// Trusted FFI seam `Tasks::poll_round` dispatches into (`extern "C" fn
// poll_task`) — unused by this probe (it only exercises the deadline/expire/
// slept_status path, not `poll_round` dispatch), but the executor's `#[no_std]`
// build still requires the symbol to exist for the crate to link.
#[no_mangle]
pub extern "C" fn poll_task(_id: u32) -> u32 {
    1
}

macro_rules! fail {
    ($($t:tt)*) => {{
        hprintln!($($t)*);
        debug::exit(debug::EXIT_FAILURE);
        loop {}
    }};
}

/// Arbitrary tick constant standing in for one second of the timer provider's
/// tick domain (Task 4) — the probe only cares about the RELATIVE ordering of
/// `now` vs. the deadline, not any real-world time unit.
const ONE_SEC: u64 = 1_000_000;
const NOW: u64 = 0;

#[entry]
fn main() -> ! {
    hprintln!("gust-timer-probe: demonstrating tickless sleep/expire/slept on the verified gust:os executor");

    let mut tasks = Tasks::new();

    // ---- 1) admit a fresh Pending task -------------------------------------
    let h = tasks.admit(0);
    if h >= MAX_TASKS as u32 {
        fail!("gust-timer-probe FAIL: admit() returned invalid handle {:#x} (table unexpectedly full)", h);
    }
    if !matches!(tasks.state[h as usize], TaskState::Pending) {
        fail!("gust-timer-probe FAIL: admit() left task {} not Pending", h);
    }

    // ---- 2) set_deadline + BEFORE assertion --------------------------------
    let deadline = NOW + ONE_SEC;
    tasks.set_deadline(h, deadline);
    if tasks.deadline[h as usize] != deadline {
        fail!(
            "gust-timer-probe FAIL: set_deadline did not stick — deadline[{}]={} want {}",
            h,
            tasks.deadline[h as usize],
            deadline
        );
    }
    let status_before = tasks.slept_status(h, NOW);
    if status_before != 0 {
        fail!(
            "gust-timer-probe FAIL: slept_status(h, NOW)={} want 0 (pending) before the deadline",
            status_before
        );
    }

    // ---- 3) TICKLESS assertion: exactly one armed alarm --------------------
    // next_deadline() is the single instant the outer layer would arm a
    // one-shot HW alarm for — no periodic tick. With only one Pending task,
    // it must equal that task's deadline exactly.
    let nd = tasks.next_deadline();
    if nd != deadline {
        fail!(
            "gust-timer-probe FAIL: next_deadline()={} want {} (the single armed alarm — tickless property violated)",
            nd,
            deadline
        );
    }

    // ---- 4) NO PREMATURE WAKE: expire() just before the deadline is a no-op
    let just_before = deadline - 1;
    tasks.expire(just_before);
    if tasks.is_ready(h) {
        fail!(
            "gust-timer-probe FAIL: expire(deadline-1) readied task {} prematurely (ready={:#x})",
            h,
            tasks.ready
        );
    }
    if !matches!(tasks.state[h as usize], TaskState::Pending) {
        fail!(
            "gust-timer-probe FAIL: expire(deadline-1) changed task {} out of Pending (no-spin violated)",
            h
        );
    }
    let status_just_before = tasks.slept_status(h, just_before);
    if status_just_before != 0 {
        fail!(
            "gust-timer-probe FAIL: slept_status(h, deadline-1)={} want 0 (still pending, no premature wake)",
            status_just_before
        );
    }

    // ---- 5) WAKE AT DEADLINE: expire() exactly at the deadline readies it --
    tasks.expire(deadline);
    if !tasks.is_ready(h) {
        fail!(
            "gust-timer-probe FAIL: expire(deadline) did NOT ready task {} (ready={:#x})",
            h,
            tasks.ready
        );
    }
    let status_at = tasks.slept_status(h, deadline);
    if status_at != 1 {
        fail!(
            "gust-timer-probe FAIL: slept_status(h, deadline)={} want 1 (elapsed)",
            status_at
        );
    }

    // ---- 6) NON-VACUITY -----------------------------------------------------
    // (a) an out-of-range handle is genuinely rejected, not accidentally
    // matching the "elapsed" branch.
    let invalid_h = MAX_TASKS as u32;
    let status_invalid = tasks.slept_status(invalid_h, deadline);
    if status_invalid != 0xFFFF_FFFF {
        fail!(
            "gust-timer-probe FAIL: slept_status(invalid handle {})={:#x} want 0xFFFF_FFFF",
            invalid_h,
            status_invalid
        );
    }
    // (b) a freshly-admitted task defaults to deadline == u64::MAX (Tasks::new's
    // initializer) and so reports pending — the "0" result above is a genuine
    // not-yet-due outcome, not the function's only reachable value.
    let mut fresh = Tasks::new();
    let hf = fresh.admit(0);
    if hf >= MAX_TASKS as u32 {
        fail!("gust-timer-probe FAIL: non-vacuity setup — admit() on a fresh Tasks failed ({:#x})", hf);
    }
    if fresh.deadline[hf as usize] != u64::MAX {
        fail!(
            "gust-timer-probe FAIL: non-vacuity setup — fresh task deadline={} want u64::MAX",
            fresh.deadline[hf as usize]
        );
    }
    let status_fresh = fresh.slept_status(hf, deadline);
    if status_fresh != 0 {
        fail!(
            "gust-timer-probe FAIL: slept_status(fresh not-slept task, deadline)={} want 0 (pending, deadline==u64::MAX)",
            status_fresh
        );
    }

    hprintln!(
        "gust-timer-probe OK: admit->Pending, set_deadline(h, NOW+{})={} took, slept_status==0 pre-deadline, next_deadline()==deadline (single tickless alarm, no periodic tick), expire(deadline-1) no-op (no premature wake, still Pending), expire(deadline) readies h + slept_status==1 (elapsed), non-vacuity: invalid handle->0xFFFF_FFFF, fresh u64::MAX-deadline task->0 (pending)",
        ONE_SEC,
        deadline
    );
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}
