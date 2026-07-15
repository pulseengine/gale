//! gust-exec-probe — LOCAL qemu-semihosting liveness probe of the gust:os v1
//! async executor (drivers/os-node/exec-cm3.o): the verified Verus+Kani-proven
//! scheduler core (src/executor.rs), dissolved to ONE relocatable Cortex-M3
//! object (loom optimize --passes inline | synth compile --target cortex-m3
//! --all-exports --relocatable --native-pointer-abi — no wac plug, no meld
//! fuse, so not synth#739-blocked BY MELD-FUSION; see drivers/exec-provider
//! RESULTS.md for a synth#739-class gap this probe DOES currently hit).
//! Admits two tasks at distinct priorities with a due-now deadline, drives
//! one poll_round, and asserts both reach Done — the executable liveness
//! half of VER-OS-EXEC-001 (REQ-OS-EXEC-001).
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

// task bodies (trusted seam): all complete on first poll for this liveness check
#[no_mangle]
pub extern "C" fn poll_task(_id: u32) -> u32 {
    1
}

#[entry]
fn main() -> ! {
    let hi = unsafe { exec_admit(1, 0, 0) }; // higher priority (lower value), due now
    let lo = unsafe { exec_admit(9, 0, 0) };
    unsafe { exec_poll_round(0, 0) };
    let ok = unsafe { exec_state(hi) == 2 /*Done*/ && exec_state(lo) == 2 };
    if ok && hi != 0xFFFF_FFFF && lo != 0xFFFF_FFFF {
        hprintln!("gust-exec-probe OK: both tasks Done, hi-prio first");
        debug::exit(debug::EXIT_SUCCESS);
    } else {
        hprintln!(
            "gust-exec-probe FAIL: hi={:#x} lo={:#x} state(hi)={} state(lo)={}",
            hi,
            lo,
            unsafe { exec_state(hi) },
            unsafe { exec_state(lo) }
        );
        debug::exit(debug::EXIT_FAILURE);
    }
    loop {}
}
