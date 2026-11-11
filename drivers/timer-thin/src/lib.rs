//! gust:hal **thin-seam** hardware-timer driver — gust-OS v0.3.0 driver breadth.
//!
//! The STM32 general-purpose timer protocol (prescaler/auto-reload config, enable,
//! read the counter, ack the update flag) AND the **wrap-safe deadline arithmetic**
//! that makes the counter a usable time capability live here, in verified wasm. It
//! imports ONLY `gust:hal/mmio` — same as gpio-thin, so **zero new TCB atoms**. The
//! deadline math is the verifiable core: Kani-proven free of wrap-induced missed or
//! early fires across the full 32-bit tick domain. TABLE-FREE by construction (no
//! `match`/array → no `.rodata` linmem lookup; see gpio-thin's lesson): all logic is
//! arithmetic, so it dissolves `--relocatable` with 0 SRAM / 0 linmem.
//!
//! REQ-DRV-COMPONENT-001: this is a wasm **component** — `world timer-driver`
//! (`../wit/gust-hal.wit`), importing `gust:hal/mmio` and exporting `gust:hal/timer`.
//! The capability is therefore checked against a typed contract at composition time,
//! instead of being an untyped `env` extern that only had to match by name at link.
//!
//! Build:   cargo build --release --target wasm32-unknown-unknown
//!          wasm-tools component new <wasm> -o timer.component.wasm
//! Dissolve: loom optimize --passes inline | synth compile --target cortex-m3
//!           --all-exports --relocatable
//! Verify:  cargo kani   (the wrap-safe deadline core)
#![cfg_attr(not(kani), no_std)]

#[cfg(not(kani))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

// wit-bindgen's canonical-ABI glue must LINK against a global allocator; this world
// is scalar-only (u32 in, u32 out), so nothing ever calls it and a zero-state
// trapping allocator keeps the driver's 0-SRAM property intact.
#[cfg(not(kani))]
use core::alloc::{GlobalAlloc, Layout};
#[cfg(not(kani))]
struct NoAlloc;
#[cfg(not(kani))]
unsafe impl GlobalAlloc for NoAlloc {
    unsafe fn alloc(&self, _: Layout) -> *mut u8 {
        core::ptr::null_mut()
    }
    unsafe fn dealloc(&self, _: *mut u8, _: Layout) {}
}
#[cfg(not(kani))]
#[global_allocator]
static ALLOC: NoAlloc = NoAlloc;

// gust:hal mmio capability — now a WIT-typed component import (still the SAME two
// primitives the ~10-line TCB bridge gpio-thin/uart-thin use; no irq atom).
#[cfg(not(kani))]
wit_bindgen::generate!({ world: "timer-driver", path: "../wit", generate_all });
#[cfg(not(kani))]
use crate::gust::hal::mmio::{read32, write32};

// Kani model-checks the pure deadline core on the host, where no wasm bindings
// exist. No harness reaches an mmio call (they were undefined `extern` symbols
// before this seam change, equally unreachable), so a stub stands in for the import.
#[cfg(kani)]
fn read32(_addr: u32) -> u32 {
    0
}
#[cfg(kani)]
fn write32(_addr: u32, _val: u32) {}

// STM32 general-purpose timer register map (offsets from a TIM base, e.g.
// TIM2 = 0x4000_0000). Device knowledge as *data*, not trusted code.
const CR1: u32 = 0x00; // control 1  (CEN = bit 0)
const SR: u32 = 0x10; // status     (UIF = bit 0, rc_w0)
const CNT: u32 = 0x24; // counter
const PSC: u32 = 0x28; // prescaler
const ARR: u32 = 0x2C; // auto-reload
const CEN: u32 = 1 << 0; // counter enable
const UIF: u32 = 1 << 0; // update interrupt flag

/// The driver's pure, verifiable core: has the wrapping counter reached `deadline`?
///
/// A naive `now >= deadline` mis-fires across the u32 wrap boundary. The correct
/// monotonic-within-half-range test treats the difference as signed: `now` is at or
/// past `deadline` iff `(now - deadline)` interpreted as i32 is ≥ 0. For any deadline
/// set as `start + interval` with `interval < 2^31`, this fires exactly when the
/// elapsed ticks reach the interval — including across a wrap — and never a tick early
/// or late (Kani-proven below). Pure arithmetic ⇒ table-free ⇒ 0 linmem.
#[inline]
pub fn has_elapsed(now: u32, deadline: u32) -> bool {
    (now.wrapping_sub(deadline) as i32) >= 0
}

#[inline(always)]
fn rd(a: u32) -> u32 {
    read32(a)
}
#[inline(always)]
fn wr(a: u32, v: u32) {
    write32(a, v)
}

// ---- exported protocol primitives (scalar ABI, 0 SRAM, 0 linmem) ----

/// Configure the timer at `base`: prescaler `psc`, auto-reload `arr`, and start it.
#[no_mangle]
pub extern "C" fn timer_init(base: u32, psc: u32, arr: u32) {
    wr(base + PSC, psc);
    wr(base + ARR, arr);
    wr(base + CR1, CEN);
}

/// Read the timer's current counter value.
#[no_mangle]
pub extern "C" fn timer_now(base: u32) -> u32 {
    rd(base + CNT)
}

/// Compute the absolute deadline `ticks` from `now` (wrapping — the caller compares
/// with `timer_elapsed`). Pure; exposed so the app owns the schedule.
#[no_mangle]
pub extern "C" fn timer_deadline(now: u32, ticks: u32) -> u32 {
    now.wrapping_add(ticks)
}

/// 1 if the wrapping counter `now` has reached `deadline` (wrap-safe), else 0.
#[no_mangle]
pub extern "C" fn timer_elapsed(now: u32, deadline: u32) -> u32 {
    has_elapsed(now, deadline) as u32
}

/// Acknowledge (clear) the update flag — write 0 to UIF (rc_w0), 1 elsewhere (no-op).
#[no_mangle]
pub extern "C" fn timer_ack(base: u32) {
    wr(base + SR, !UIF);
}

// `gust:hal/timer` exported over the SAME bodies as the C-ABI symbols above, not a
// second implementation: the component's exports and the dissolved object's
// `timer_*` entry points (benches/gust/build.rs, gust_timer probe) then cannot
// diverge in behaviour. Same shape gpio-thin used.
#[cfg(not(kani))]
struct Driver;
#[cfg(not(kani))]
impl exports::gust::hal::timer::Guest for Driver {
    fn init(base: u32, psc: u32, arr: u32) {
        timer_init(base, psc, arr)
    }
    fn now(base: u32) -> u32 {
        timer_now(base)
    }
    fn deadline(now: u32, ticks: u32) -> u32 {
        timer_deadline(now, ticks)
    }
    fn elapsed(now: u32, deadline: u32) -> u32 {
        timer_elapsed(now, deadline)
    }
    fn ack(base: u32) {
        timer_ack(base)
    }
}
#[cfg(not(kani))]
export!(Driver);

/// Kani proofs for the verifiable core (`cargo kani`): the deadline test is wrap-safe.
#[cfg(kani)]
mod kani_proofs {
    use super::*;

    /// THE property: for a deadline set as `start + interval` (interval < 2^31),
    /// as `now` advances `elapsed` ticks from start (elapsed < 2^31), the timer fires
    /// EXACTLY when `elapsed >= interval` — including across the u32 wrap boundary, so
    /// no deadline is missed or fired early because the counter wrapped.
    #[kani::proof]
    fn no_wrap_induced_misfire() {
        let start: u32 = kani::any();
        let interval: u32 = kani::any();
        let elapsed: u32 = kani::any();
        kani::assume(interval < (1u32 << 31));
        kani::assume(elapsed < (1u32 << 31));
        let deadline = start.wrapping_add(interval);
        let now = start.wrapping_add(elapsed);
        assert_eq!(has_elapsed(now, deadline), elapsed >= interval);
    }

    /// A deadline equal to the current count has elapsed (fires, not skipped).
    #[kani::proof]
    fn reflexive_fires() {
        let t: u32 = kani::any();
        assert!(has_elapsed(t, t));
    }

    /// The exported predicate is a clean 0/1 boolean (no register garbage in the ABI).
    #[kani::proof]
    fn export_is_boolean() {
        let now: u32 = kani::any();
        let deadline: u32 = kani::any();
        let e = timer_elapsed(now, deadline);
        assert!(e == 0 || e == 1);
    }
}
