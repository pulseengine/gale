//! gust:os `timer` provider (world `timer-provider`, wit-os/gust-os.wit) — Task 4.
//! STATELESS by construction, and clockless: it owns neither the deadline table nor
//! the clock. `sleep`/`slept` operate on the app's handle UNCHANGED against the
//! node's single executor instance via `gust:sched/tasks`, and read time through
//! `gust:os/time` — so this crate has no `gust:hal` import at all.
//!
//! Both halves used to be local copies: an embedded `plain/src/executor.rs` with its
//! own `static mut TASKS` (a deadline table no other provider could see, so
//! `timer.sleep(h)` armed a wake on a task `spawn.start` had never created), and a
//! hardcoded `TIM2_CNT` read that agreed with `time.now()` only because the two
//! constants happened to match. Importing both capabilities makes one scheduler and
//! one clock structural rather than coincidental.
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
use crate::gust::os::time;
use crate::gust::sched::tasks;
use exports::gust::os::timer::Guest;

struct P;
impl Guest for P {
    /// Arm a one-shot wake `ticks` from now on task `handle` — the RESOLVED
    /// SEAM (v0.4.0 timer-sleep spec, task-4 brief): reject out-of-range
    /// `ticks` (`>= 2^31`) up front; otherwise compute the deadline with
    /// `time.deadline` (the Kani-proven wrap-safe add this node already
    /// publishes, rather than a second `wrapping_add` written here) and hand it
    /// to the verified `Tasks::set_deadline`, which itself only writes a valid
    /// Pending slot's deadline (Kani-framed by `set_deadline_sets_only_h`: a
    /// Free/Done/out-of-range handle is a no-op) — so this marshalling layer
    /// re-implements no admission decision and no arithmetic, only the
    /// ticks-range guard and the u64 lo/hi split the seam carries.
    fn sleep(handle: u32, ticks: u32) -> u32 {
        // ticks is u32 at the seam (v1 bound < 2^31 fits u32) — a u64 seam param
        // makes synth's ARM backend loud-decline the frame-backing sleep (u64 param
        // + a call); widening internally keeps the deadline math full-width.
        if ticks >= (1u32 << 31) {
            return 0xFFFF_FFFF;
        }
        let d = time::deadline(time::now(), ticks as u64);
        tasks::set_deadline(handle, d as u32, (d >> 32) as u32);
        0
    }

    /// Poll a timer handle: delegate directly to the verified
    /// `slept_status(handle, now())` — 0 pending / 1 elapsed / 0xFFFF_FFFF
    /// invalid, unchanged.
    fn slept(handle: u32) -> u32 {
        let now = time::now();
        tasks::slept_status(handle, now as u32, (now >> 32) as u32)
    }
}
export!(P);
