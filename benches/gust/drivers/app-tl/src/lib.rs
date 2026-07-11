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

wit_bindgen::generate!({ world: "app-tl", path: "../wit-os", generate_all });
use crate::gust::os::time::{now, deadline, elapsed};
use crate::gust::os::log::line;
use alloc::vec::Vec;

struct App;
impl Guest for App {
    fn run() -> u32 {
        let mut m: Vec<u8> = Vec::new();
        m.extend_from_slice(b"gust:os up\n");
        line(&m);
        let t0 = now();
        let d = deadline(t0, 100);
        if d == t0.wrapping_add(100) && !elapsed(t0, d) && elapsed(d, d) { 0xA } else { 0xBAD }
    }
}
export!(App);
