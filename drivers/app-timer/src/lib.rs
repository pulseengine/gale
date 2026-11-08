//! gust:os step-4 app (world `app-timer`, wit-os/gust-os.wit): imports gust:os
//! {time, spawn, timer} and demonstrates THE PERIODIC-LOOP SHAPE — arm a one-shot
//! wake on a real task handle, then poll it to completion.
//!
//! This is the worked example gale#223 asked for. The declared world previously
//! imported only {time, timer}, which cannot work: `timer.sleep` takes THE HANDLE
//! THE APP HOLDS FROM `spawn.start`, so without `spawn` an app here has no way to
//! obtain a valid handle and can only exercise sleep's 0xFFFF_FFFF invalid-handle
//! path. `spawn` was added to the world when this example was written.
//!
//! The shape, and why each step is what it is:
//!
//!   1. `spawn.start(entry)` -> a handle on the ONE executor instance. The spawn
//!      provider is stateless; the handle is the executor's own, which is why it
//!      means the same thing to `timer.sleep` and `exec.state`.
//!   2. `timer.sleep(h, ticks)` -> arms a one-shot wake `ticks` from now on that
//!      handle, computed against `time`'s clock rather than a second register
//!      read. Returns 0 on success; 0xFFFF_FFFF if the handle is invalid, not
//!      Pending, or `ticks >= 2^31`.
//!   3. `timer.slept(h)` -> 0 pending / 1 elapsed / 0xFFFF_FFFF invalid.
//!
//! WHAT THIS DOES NOT SHOW. It does not drive a schedule: nothing here calls
//! `exec.poll-round`, because who drives the executor is the EMBEDDER's choice
//! (timer ISR vs partition window) and gale has not settled it — see gale#223.
//! An app arms its wake and reports; the node decides when to poll. Sizing a
//! real periodic budget additionally needs T4 static WCET (3 of 31 functions
//! bounded today) and must account for the switch tick: a partition owning k
//! windows receives Theta - k, not Theta (proofs/lean/PartitionSupply.lean).
#![no_std]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { loop {} }
extern crate alloc;
use core::alloc::{GlobalAlloc, Layout};
const ARENA: usize = 512;
static mut HEAP: [u8; ARENA] = [0; ARENA];
static mut OFF: usize = 0;
struct Bump;
unsafe impl GlobalAlloc for Bump {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let a=l.align(); let o=(OFF+(a-1))&!(a-1);
        if o+l.size()>ARENA { return core::ptr::null_mut(); }
        OFF=o+l.size(); (&raw mut HEAP as *mut u8).add(o)
    }
    unsafe fn dealloc(&self,_:*mut u8,_:Layout){}
}
#[global_allocator] static A: Bump = Bump;

wit_bindgen::generate!({ world: "gust:os/app-timer@0.1.0", path: ["../wit", "../wit-os"], generate_all });
use crate::gust::os::spawn::start;
use crate::gust::os::time::now;
use crate::gust::os::timer::{sleep, slept};

/// Wake this many ticks out. Well under the 2^31 bound `sleep` enforces.
const WAKE_TICKS: u32 = 50;
/// Bounded poll budget — an app must not spin unboundedly inside its window.
const MAX_POLLS: u32 = 4;

struct App;
impl Guest for App {
    /// 1 = the armed wake was observed to elapse. 0 = still pending after the
    /// budget. 0xFFFF_FFFF = the arm was rejected (invalid handle / out of range),
    /// returned UNCHANGED rather than folded into 0, so a caller can tell "not yet"
    /// from "never armed".
    fn run() -> u32 {
        // Touch the clock so the seam carries all three imports end-to-end, and so
        // the deadline below is computed against the same `time` the timer uses.
        let _t0 = now();

        let h = start(0);
        let armed = sleep(h, WAKE_TICKS);
        if armed != 0 {
            return armed; // 0xFFFF_FFFF — propagate, do not mask
        }

        let mut r: u32 = 0;
        let mut i: u32 = 0;
        while i < MAX_POLLS {
            r = slept(h);
            if r != 0 { break; } // 1 = elapsed, 0xFFFF_FFFF = invalid
            i += 1;
        }
        r
    }
}
export!(App);
