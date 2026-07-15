//! gust-exec-probe — LOCAL qemu-semihosting liveness probe of the gust:os v1
//! async executor (drivers/os-node/exec-cm3.o): the verified Verus+Kani-proven
//! scheduler core (src/executor.rs), dissolved to ONE relocatable Cortex-M3
//! object (loom optimize --passes inline | synth compile --target cortex-m3
//! --all-exports --relocatable --native-pointer-abi — no wac plug, no meld
//! fuse, so not synth#739-blocked BY MELD-FUSION; see drivers/exec-provider
//! RESULTS.md for a synth#739-class gap this probe DOES currently hit).
//! Admits two tasks at distinct priorities with a due-now deadline, drives
//! one poll_round, and asserts both reach Done AND that pick_next's proven
//! priority ordering was actually observed at dispatch (the `poll_task` FFI
//! seam records the id argument's call order; see `ORDER` below) — the
//! executable liveness half of VER-OS-EXEC-001 (REQ-OS-EXEC-001).
#![no_std]
#![no_main]
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use panic_halt as _;

// ABI note (synth#518 — see drivers/exec-provider/src/lib.rs for the full
// account): the dissolved object's `exec_admit`/`exec_poll_round` take a
// deadline/now argument as separate lo/hi u32 halves rather than one u64. A
// wasm export combining a 64-bit param with a (post loom-inline) call hits a
// synth ARM-backend gap that silently drops the whole function from the
// dissolved object; splitting avoids it. `exec_admit` uses plain sequential
// u32 params too (not a mixed u32-then-u64 list) so both sides of the FFI
// boundary use the SAME simple register convention — no dependence on synth
// correctly reproducing AAPCS's 64-bit register-pair alignment padding.
extern "C" {
    fn exec_admit(prio: u32, deadline_lo: u32, deadline_hi: u32) -> u32;
    fn exec_poll_round(now_lo: u32, now_hi: u32);
    fn exec_state(h: u32) -> u32;
}

// Dispatch-order log: `poll_task` is the ONLY point the dissolved executor
// hands control back to this probe (`Tasks::dispatch_one`'s trusted FFI
// seam), so recording the `id` argument here — in call order — is a direct,
// genuine observation of the order `poll_round`'s `pick_next` loop dispatched
// tasks in, not an assumption. Plain `static mut` (no atomics/locks): this
// probe is single-threaded, run-to-completion inside one `exec_poll_round`
// call, matching the `static mut TASKS` singleton pattern the dissolved
// object itself uses (see exec-provider/src/lib.rs).
const ORDER_CAP: usize = 8;
static mut ORDER: [u32; ORDER_CAP] = [0xFFFF_FFFF; ORDER_CAP];
static mut ORDER_LEN: usize = 0;

// task bodies (trusted seam): all complete on first poll for this liveness check
#[no_mangle]
pub extern "C" fn poll_task(id: u32) -> u32 {
    unsafe {
        if ORDER_LEN < ORDER_CAP {
            ORDER[ORDER_LEN] = id;
            ORDER_LEN += 1;
        }
    }
    1
}

#[entry]
fn main() -> ! {
    let hi = unsafe { exec_admit(1, 0, 0) }; // higher priority (lower value), due now
    let lo = unsafe { exec_admit(9, 0, 0) };
    unsafe { exec_poll_round(0, 0) };
    let done = unsafe { exec_state(hi) == 2 /*Done*/ && exec_state(lo) == 2 };
    // Genuinely OBSERVED dispatch order (not assumed): pick_next scans for
    // the lowest `prio` value among ready tasks (Priority convention: lower
    // = higher priority), so admitting `hi` at prio 1 and `lo` at prio 9,
    // both due now, means poll_round's first dispatch — and thus the first
    // recorded `poll_task` call — must be `hi`. Checking ORDER here is what
    // makes the "hi-prio first" claim below a tested fact rather than an
    // untested assumption from both tasks merely reaching Done.
    let order_ok = unsafe { ORDER_LEN == 2 && ORDER[0] == hi && ORDER[1] == lo };
    let ok = done && order_ok && hi != 0xFFFF_FFFF && lo != 0xFFFF_FFFF;
    if ok {
        hprintln!("gust-exec-probe OK: both tasks Done, hi-prio first (dispatch order observed)");
        debug::exit(debug::EXIT_SUCCESS);
    } else {
        hprintln!(
            "gust-exec-probe FAIL: hi={:#x} lo={:#x} state(hi)={} state(lo)={} order={:?} order_len={}",
            hi,
            lo,
            unsafe { exec_state(hi) },
            unsafe { exec_state(lo) },
            unsafe { ORDER },
            unsafe { ORDER_LEN },
        );
        debug::exit(debug::EXIT_FAILURE);
    }
    loop {}
}
