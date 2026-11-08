#![no_std]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { loop {} }

// wit-bindgen's canonical-ABI glue requires a global allocator to LINK, but a
// scalar-only (u32↔u32) interface never calls it at runtime — so a zero-state
// trapping allocator satisfies the requirement while keeping .bss/.data = 0.
use core::alloc::{GlobalAlloc, Layout};
struct NoAlloc;
unsafe impl GlobalAlloc for NoAlloc {
    unsafe fn alloc(&self, _: Layout) -> *mut u8 { core::ptr::null_mut() }
    unsafe fn dealloc(&self, _: *mut u8, _: Layout) {}
}
#[global_allocator]
static ALLOC: NoAlloc = NoAlloc;

wit_bindgen::generate!({ world: "gpio-driver", path: "../wit" });

use crate::gust::hal::mmio::{read32, write32};
use exports::gust::hal::gpio::Guest;

const NIBBLE_LUT: u32 = 0x0B63_2840;
const CRL: u32 = 0x00; const CRH: u32 = 0x04; const IDR: u32 = 0x08;
const ODR: u32 = 0x0C; const BSRR: u32 = 0x10;

fn nibble_for_idx(i: u32) -> u32 { if i > 6 { 0 } else { (NIBBLE_LUT >> (i * 4)) & 0xF } }
fn pin_slot(pin: u32) -> (u32, u32) { let p = pin & 0xF; if p < 8 { (CRL, p*4) } else { (CRH, (p-8)*4) } }

struct Driver;
impl Guest for Driver {
    fn configure(port_base: u32, pin: u32, mode_idx: u32) {
        let (reg, shift) = pin_slot(pin);
        let nib = nibble_for_idx(mode_idx);
        let cur = read32(port_base + reg);
        write32(port_base + reg, (cur & !(0xF << shift)) | (nib << shift));
    }
    fn set(port_base: u32, pin: u32) { write32(port_base + BSRR, 1 << (pin & 0xF)); }
    fn clear(port_base: u32, pin: u32) { write32(port_base + BSRR, 1 << ((pin & 0xF) + 16)); }
    fn read(port_base: u32, pin: u32) -> u32 { (read32(port_base + IDR) >> (pin & 0xF)) & 1 }
    fn toggle(port_base: u32, pin: u32) {
        let p = pin & 0xF;
        if (read32(port_base + ODR) >> p) & 1 != 0 { write32(port_base + BSRR, 1 << (p+16)); }
        else { write32(port_base + BSRR, 1 << p); }
    }
}
export!(Driver);
