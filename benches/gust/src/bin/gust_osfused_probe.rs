//! gust-osfused-probe — the E2 FUSED `gust:os` composite EXECUTED under qemu
//! (lm3s6965evb, real v7-M), reporting over semihosting.
//!
//! Distinct from `gust_os_probe`, which drives the v0.4.0 STEP-1 node
//! (`os-time-cm3.o`, one capability). This drives
//! `drivers/os-node/gustos-dissolved-cm3.o` — the flagship of the E2 ladder, the
//! WHOLE `gust:os` seam (time, log, spawn, exec, timer, sched/tasks) fused from a
//! component graph into ONE Cortex-M3 object with three native atoms.
//!
//! That object is built by `drivers/build-dissolve-gustos.sh`, gated on its
//! undefined-symbol set and its size, and measured for WCET — but it was linked
//! into **nothing**, so it had never been executed.
//!
//! Which matters, because it is the artifact with a KNOWN defect. synth 0.56.0
//! REFUSES to emit `gust:os/timer@0.1.0#sleep` from this input (gale#269,
//! synth#929): `sleep` calls `time#deadline(now: u64, ticks: u64)`, and on
//! thumb-2 an i64 argument that is not last is marshalled into the wrong
//! registers — the high half dropped, every later argument shifted down. Our
//! pinned synth 0.52.0 emits it anyway, silently, and that object is committed.
//!
//! `os-deadline-armed` is the point: arm a one-shot timer and watch whether it
//! fires at the right time. A DIRECTLY called `deadline` crosses the native AAPCS
//! boundary and is expected to be correct — only the call synth generates INSIDE
//! `sleep` is mismarshalled. Both are checked, separately, so the difference is
//! visible instead of averaged away.
#![no_std]
#![no_main]
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use panic_halt as _;

// ── the three native atoms ──────────────────────────────────────────────────────
// A software tick source: `time#now` reads it through the mmio seam and widens it
// to u64, so the probe controls time exactly and nothing depends on a wall clock.
static mut TICKS: u32 = 0;

#[export_name = "read32"]
pub extern "C" fn seam_read32(_addr: u32) -> u32 {
    unsafe { core::ptr::read_volatile(&raw const TICKS) }
}

#[export_name = "write32"]
pub extern "C" fn seam_write32(_addr: u32, _val: u32) {}

static mut POLLED: u32 = 0;

#[export_name = "poll-task"]
pub extern "C" fn seam_poll_task(_id: u32) -> u32 {
    unsafe {
        core::ptr::write_volatile(&raw mut POLLED, core::ptr::read_volatile(&raw const POLLED) + 1)
    };
    1 // Done
}

extern "C" {
    #[link_name = "gust:os/time@0.1.0#now"]
    fn os_now() -> u64;
    #[link_name = "gust:os/time@0.1.0#deadline"]
    fn os_deadline(now: u64, ticks: u64) -> u64;
    #[link_name = "gust:os/time@0.1.0#elapsed"]
    fn os_elapsed(now: u64, deadline: u64) -> i32;
    #[link_name = "gust:os/spawn@0.1.0#start"]
    fn os_spawn_start(entry: u32) -> u32;
    #[link_name = "gust:os/timer@0.1.0#sleep"]
    fn os_sleep(handle: u32, ticks: u32) -> u32;
    #[link_name = "gust:os/timer@0.1.0#slept"]
    fn os_slept(handle: u32) -> u32;
    #[link_name = "gust:sched/tasks@0.1.0#poll-round"]
    fn os_poll_round(now_lo: u32, now_hi: u32);
}

static mut FAILED: u32 = 0;

fn ok(cond: bool, good: &str, bad: &str) {
    if !cond {
        unsafe {
            core::ptr::write_volatile(
                &raw mut FAILED,
                core::ptr::read_volatile(&raw const FAILED) + 1,
            )
        };
    }
    hprintln!("{}", if cond { good } else { bad });
}

fn set_ticks(t: u32) {
    unsafe { core::ptr::write_volatile(&raw mut TICKS, t) };
}

#[entry]
fn main() -> ! {
    unsafe {
        hprintln!("osfused-gate begin");

        // 0) The mmio seam is real: `now` tracks the tick source through it.
        set_ticks(100);
        let n1 = os_now();
        set_ticks(250);
        let n2 = os_now();
        ok(n1 == 100 && n2 == 250, "osfused-now-ok", "osfused-now-bad");

        // 1) `deadline` called DIRECTLY — native AAPCS, where a u64 argument is
        //    passed correctly in an even-aligned register pair. Expected to hold
        //    even on the miscompiled object; this is the CONTROL for check 3.
        let d = os_deadline(100, 50);
        ok(d == 150, "osfused-deadline-direct-ok", "osfused-deadline-direct-bad");

        // 2) `elapsed` — the other (i64, i64) export, also called natively.
        ok(
            os_elapsed(140, 150) == 0 && os_elapsed(160, 150) != 0,
            "osfused-elapsed-ok",
            "osfused-elapsed-bad",
        );

        // 3) THE ONE THAT MATTERS (gale#269 / synth#929). `sleep` is all-u32 at
        //    the WIT surface, but it CALLS `deadline(now: u64, ticks: u64)` — the
        //    (i64, i64) shape thumb-2 mismarshals. If the armed deadline is wrong,
        //    the timer fires at the wrong tick, or never.
        set_ticks(1000);
        let h = os_spawn_start(0);
        let armed = os_sleep(h, 50); // due at tick 1050
        let before = os_slept(h);

        set_ticks(1010); // 40 ticks early — must still be pending
        os_poll_round(1010, 0);
        let early = os_slept(h);

        set_ticks(1100); // past the deadline — must have elapsed
        os_poll_round(1100, 0);
        let late = os_slept(h);

        hprintln!(
            "  [sleep] handle={} armed={} before={} at+10={} at+100={}",
            h, armed, before, early, late
        );
        ok(
            armed == 0 && early == 0 && late == 1,
            "osfused-deadline-armed-ok",
            "osfused-deadline-armed-bad",
        );

        hprintln!("osfused-gate done");
        let failed = core::ptr::read_volatile(&raw const FAILED);
        if failed == 0 {
            hprintln!("osfused-probe: ALL CHECKS PASSED");
            debug::exit(debug::EXIT_SUCCESS);
        } else {
            hprintln!("osfused-probe: {} CHECK(S) FAILED", failed);
            debug::exit(debug::EXIT_FAILURE);
        }
    }
    loop {
        cortex_m::asm::wfi();
    }
}
