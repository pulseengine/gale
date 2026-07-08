#![no_std]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { loop {} }
use core::alloc::{GlobalAlloc, Layout};
struct NoAlloc;
unsafe impl GlobalAlloc for NoAlloc { unsafe fn alloc(&self,_:Layout)->*mut u8{core::ptr::null_mut()} unsafe fn dealloc(&self,_:*mut u8,_:Layout){} }
#[global_allocator] static ALLOC: NoAlloc = NoAlloc;

wit_bindgen::generate!({ world: "uart-driver", path: "../wit" });
use crate::gust::hal::mmio::{read32, write32};
use crate::gust::hal::irq::poll as irq_poll;
use exports::gust::hal::uart::Guest;

const U:u32=0x4001_3800; const SR:u32=U+0x00; const DR:u32=U+0x04; const BRR:u32=U+0x08; const CR1:u32=U+0x0C;
const TXE:u32=1<<7; const RXNE:u32=1<<5; const ORE:u32=1<<3; const FE:u32=1<<1;
const UE:u32=1<<13; const TE:u32=1<<3; const RE:u32=1<<2; const RX_NONE:u32=0xFFFF_FFFF;
fn rx_ready(sr:u32)->bool { sr&ORE==0 && sr&FE==0 && sr&RXNE!=0 }

struct Driver;
impl Guest for Driver {
    fn init(brr:u32){ write32(BRR,brr); write32(CR1, UE|TE|RE); }
    fn tx_byte(b:u32){ while read32(SR)&TXE==0{} write32(DR,b&0xFF); }
    fn rx()->u32{ if rx_ready(read32(SR)) { read32(DR)&0xFF } else { RX_NONE } }
    fn rx_fired()->u32{ irq_poll(0) as u32 }
}
export!(Driver);
