#![no_std]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { loop {} }
extern crate alloc;
use core::alloc::{GlobalAlloc, Layout};
// A tiny bump allocator over a fixed static arena — the canonical ABI needs a real
// allocator once buffers cross the seam. Bounded (ARENA bytes), counts toward the
// node's SRAM budget. No free (bump only); fine for the per-call log lifetime.
const ARENA: usize = 1024;
static mut HEAP: [u8; ARENA] = [0; ARENA];
static mut OFF: usize = 0;
struct Bump;
unsafe impl GlobalAlloc for Bump {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let a = l.align(); let o = (OFF + (a-1)) & !(a-1);
        if o + l.size() > ARENA { return core::ptr::null_mut(); }
        OFF = o + l.size(); (&raw mut HEAP as *mut u8).add(o)
    }
    unsafe fn dealloc(&self, _: *mut u8, _: Layout) {}
}
#[global_allocator] static A: Bump = Bump;

wit_bindgen::generate!({ world: "log-provider", path: "../wit-os", generate_all });
use crate::gust::hal::mmio::write32;
use exports::gust::os::log::Guest;

const UART_DR: u32 = 0x4001_3804; // USART1 data register (via gust:hal/mmio)

struct L;
impl Guest for L {
    fn line(msg: alloc::vec::Vec<u8>) {
        for &b in msg.iter() { write32(UART_DR, b as u32); }
    }
}
export!(L);
