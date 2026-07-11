#![no_std]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { loop {} }
use core::alloc::{GlobalAlloc, Layout};
struct NoAlloc;
unsafe impl GlobalAlloc for NoAlloc { unsafe fn alloc(&self,_:Layout)->*mut u8{core::ptr::null_mut()} unsafe fn dealloc(&self,_:*mut u8,_:Layout){} }
#[global_allocator] static ALLOC: NoAlloc = NoAlloc;

wit_bindgen::generate!({ world: "spi-driver", path: "../wit" });
use crate::gust::hal::mmio::{read32, write32};
use exports::gust::hal::spi::Guest;

const CR1:u32=0x00; const SR:u32=0x08; const DR:u32=0x0C;
const CR1_MSTR:u32=1<<2; const CR1_SPE:u32=1<<6; const CR1_SSI:u32=1<<8; const CR1_SSM:u32=1<<9;
const BR_SHIFT:u32=3; const SR_RXNE:u32=1<<0; const SR_TXE:u32=1<<1;
const SPI_FAULT:u32=0xFFFF_FFFF; const PH:u32=30; const REM:u32=(1<<PH)-1;
fn cr1_value(mode:u32,br:u32)->u32 { CR1_SPE|CR1_MSTR|CR1_SSM|CR1_SSI|((br&0b111)<<BR_SHIFT)|(mode&0b11) }

struct Driver;
impl Guest for Driver {
    fn configure(base:u32,mode:u32,br_idx:u32){ write32(base+CR1, cr1_value(mode,br_idx)); }
    fn xfer_byte(base:u32,out:u32)->u32{ while read32(base+SR)&SR_TXE==0{} write32(base+DR,out&0xFF); while read32(base+SR)&SR_RXNE==0{} read32(base+DR)&0xFF }
    fn begin(state:u32,count:u32)->u32{ let ph=state>>PH; if ph==0 && count>0 { (1<<PH)|(count&REM) } else { SPI_FAULT } }
    fn step(state:u32)->u32{ let ph=state>>PH; if ph!=1 { return SPI_FAULT } let rem=(state&REM)-1; if rem==0 { 2<<PH } else { (1<<PH)|(rem&REM) } }
    fn is_complete(state:u32)->u32{ (state>>PH==2) as u32 }
    fn abort(_state:u32)->u32{ 0 }
}
export!(Driver);
