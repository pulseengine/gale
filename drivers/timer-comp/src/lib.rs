#![no_std]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { loop {} }
use core::alloc::{GlobalAlloc, Layout};
struct NoAlloc;
unsafe impl GlobalAlloc for NoAlloc {
    unsafe fn alloc(&self, _: Layout) -> *mut u8 { core::ptr::null_mut() }
    unsafe fn dealloc(&self, _: *mut u8, _: Layout) {}
}
#[global_allocator]
static ALLOC: NoAlloc = NoAlloc;

wit_bindgen::generate!({ world: "timer-driver", path: "../wit" });
use crate::gust::hal::mmio::{read32, write32};
use exports::gust::hal::timer::Guest;

const CR1: u32 = 0x00; const SR: u32 = 0x10; const CNT: u32 = 0x24;
const PSC: u32 = 0x28; const ARR: u32 = 0x2C; const CEN: u32 = 1; const UIF: u32 = 1;

fn has_elapsed(now: u32, deadline: u32) -> bool { (now.wrapping_sub(deadline) as i32) >= 0 }

struct Driver;
impl Guest for Driver {
    fn init(base: u32, psc: u32, arr: u32) {
        write32(base + PSC, psc); write32(base + ARR, arr); write32(base + CR1, CEN);
    }
    fn now(base: u32) -> u32 { read32(base + CNT) }
    fn deadline(now: u32, ticks: u32) -> u32 { now.wrapping_add(ticks) }
    fn elapsed(now: u32, deadline: u32) -> u32 { has_elapsed(now, deadline) as u32 }
    fn ack(base: u32) { write32(base + SR, !UIF); }
}
export!(Driver);
