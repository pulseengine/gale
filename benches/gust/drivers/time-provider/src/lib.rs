#![no_std]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { loop {} }
use core::alloc::{GlobalAlloc, Layout};
struct NoAlloc;
unsafe impl GlobalAlloc for NoAlloc { unsafe fn alloc(&self,_:Layout)->*mut u8{core::ptr::null_mut()} unsafe fn dealloc(&self,_:*mut u8,_:Layout){} }
#[global_allocator] static A: NoAlloc = NoAlloc;

wit_bindgen::generate!({ world: "time-provider", path: "../wit-os", generate_all });
use crate::gust::hal::mmio::read32;
use exports::gust::os::time::Guest;

const TIM2_CNT: u32 = 0x4000_0024; // timer count register (via gust:hal/mmio)

struct P;
impl Guest for P {
    fn now() -> u64 { read32(TIM2_CNT) as u64 }
    fn deadline(now: u64, ticks: u64) -> u64 { now.wrapping_add(ticks) }
    fn elapsed(now: u64, deadline: u64) -> bool { now >= deadline }
}
export!(P);
