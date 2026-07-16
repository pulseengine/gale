//! gust:os step-3 app (world `app-ts`, wit-os/gust-os.wit): imports gust:os
//! {time, spawn} — spawn is the first EXECUTOR-BACKED capability crossing the
//! syscall seam. `run()` starts one task and polls its handle in a bounded loop
//! (<= 4 polls), returning the LAST poll result: 1 (= done) is the pass value,
//! since the spawn provider's verified executor completes the task on the first
//! `poll_round` (the node's trusted `taskdisp.poll-task` dispatch reports done).
//! Allocator/panic/profile mirror app-tl exactly.
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

wit_bindgen::generate!({ world: "app-ts", path: "../wit-os", generate_all });
use crate::gust::os::time::now;
use crate::gust::os::spawn::{poll, start};

struct App;
impl Guest for App {
    fn run() -> u32 {
        // Touch the time capability so the seam carries BOTH imports end-to-end.
        let _t0 = now();
        let h = start(0);
        let mut r: u32 = 0;
        let mut i: u32 = 0;
        while i < 4 {
            r = poll(h);
            if r == 1 { break; }
            i += 1;
        }
        r
    }
}
export!(App);
