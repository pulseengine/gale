//! gust:os `timer` provider (world `timer-provider`, wit-os/gust-os.wit) — Task 4.
//! Backs `sleep`/`slept` with the SAME verified executor deadline table
//! (`plain/src/executor.rs`, included verbatim — Task 2's `set_deadline`/
//! `slept_status` methods on `Tasks`) that `spawn-provider`/`exec-provider`
//! dissolve, not a hand-written placeholder. Unlike `spawn-provider`'s
//! `poll`, neither export here drives `poll_round`/`dispatch_one`, so this
//! crate never crosses the trusted `taskdisp`/`poll_task` FFI seam at all —
//! its only externs are the `gust:hal/mmio` reads `now()` needs (the same
//! seam `time-provider` uses).
#![no_std]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
use core::alloc::{GlobalAlloc, Layout};
struct NoAlloc;
unsafe impl GlobalAlloc for NoAlloc {
    unsafe fn alloc(&self, _: Layout) -> *mut u8 {
        core::ptr::null_mut()
    }
    unsafe fn dealloc(&self, _: *mut u8, _: Layout) {}
}
#[global_allocator]
static A: NoAlloc = NoAlloc;

wit_bindgen::generate!({ world: "timer-provider", path: "../wit-os", generate_all });
use crate::gust::hal::mmio::read32;
use exports::gust::os::timer::Guest;

#[path = "../../../../../plain/src/executor.rs"]
mod executor;
use executor::Tasks;

/// Timer count register (via `gust:hal/mmio`) — same register `time-provider`
/// reads for `gust:os/time.now`, so `sleep`'s deadline math and `time.now`
/// observe the identical monotonic source.
const TIM2_CNT: u32 = 0x4000_0024;

fn now() -> u64 {
    read32(TIM2_CNT) as u64
}

// Lazily-initialized executor deadline-table state. NOT `Option<Tasks>`: see
// spawn-provider/src/lib.rs for the .bss-vs-.data straddle rationale (the
// niche-encoded `None` discriminant is one initialized byte inside an
// otherwise-zero struct, which wasm-ld splits across the .data end / .bss
// tail — exactly the straddling-static geometry synth's --shadow-stack-size
// shrink refuses, VCR-MEM-001/#678, when this module is meld-fused into a
// node). A `MaybeUninit` table + separate flag is all-zero at init, so the
// whole table lands in .bss and the node's data segment stays clean.
//
// NOTE: this is a SEPARATE `Tasks` instance from spawn-provider's — v1 does
// not share the deadline table across providers (see RESULTS.md "Known
// limitation").
static mut TASKS_INIT: u32 = 0;
static mut TASKS: core::mem::MaybeUninit<Tasks> = core::mem::MaybeUninit::uninit();

#[allow(static_mut_refs)]
unsafe fn tasks() -> &'static mut Tasks {
    if TASKS_INIT == 0 {
        TASKS.write(Tasks::new());
        TASKS_INIT = 1;
    }
    TASKS.assume_init_mut()
}

struct P;
impl Guest for P {
    /// Arm a one-shot wake `ticks` from now on task `handle` — the RESOLVED
    /// SEAM (v0.4.0 timer-sleep spec, task-4 brief): reject out-of-range
    /// `ticks` (`>= 2^31`) up front; otherwise compute `d = now() + ticks`
    /// (wrap-safe u64 add, mirroring `time.deadline`'s `wrapping_add`) and
    /// hand it to the verified `Tasks::set_deadline`, which itself only
    /// writes a valid Pending slot's deadline (Kani-framed by
    /// `set_deadline_sets_only_h`: a Free/Done/out-of-range handle is a
    /// no-op) — so this marshalling layer re-implements no admission
    /// decision, only the wrap-safe arithmetic and the ticks-range guard.
    fn sleep(handle: u32, ticks: u64) -> u32 {
        if ticks >= (1u64 << 31) {
            return 0xFFFF_FFFF;
        }
        let d = now().wrapping_add(ticks);
        let t = unsafe { tasks() };
        t.set_deadline(handle, d);
        0
    }

    /// Poll a timer handle: delegate directly to the verified
    /// `slept_status(handle, now())` — 0 pending / 1 elapsed / 0xFFFF_FFFF
    /// invalid, unchanged.
    fn slept(handle: u32) -> u32 {
        let t = unsafe { tasks() };
        t.slept_status(handle, now())
    }
}
export!(P);
