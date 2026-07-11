#![no_std]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { loop {} }
use core::alloc::{GlobalAlloc, Layout};
struct NoAlloc;
unsafe impl GlobalAlloc for NoAlloc { unsafe fn alloc(&self,_:Layout)->*mut u8{core::ptr::null_mut()} unsafe fn dealloc(&self,_:*mut u8,_:Layout){} }
#[global_allocator] static A: NoAlloc = NoAlloc;

wit_bindgen::generate!({ world: "app-time", path: "../wit-os" });
use crate::gust::os::time::{now, deadline, elapsed};

struct App;
impl Guest for App {
    // Portable app: reads OS time, sets a deadline 100 ticks out, reports whether a
    // later now has elapsed it. Returns a checkable code (0xA=ok path).
    fn run() -> u32 {
        let t0 = now();
        let d = deadline(t0, 100);
        // d must be t0+100; not-yet-elapsed at t0, elapsed at d.
        if d == t0.wrapping_add(100) && !elapsed(t0, d) && elapsed(d, d) { 0xA } else { 0xBAD }
    }
}
export!(App);
