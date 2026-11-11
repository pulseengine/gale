//! SPIKE — gust:mpu thin-seam I-ISO region-programming core.
//!
//! The verified bodies are USED, not copied. This component depends on the
//! `gale` crate and exports `gale::mpu_switch` — the Verus+Kani-verified
//! I-ISO core — through WIT. There is no second copy of the region logic to
//! keep in step, so "lifted VERBATIM" is no longer a claim a reader has to
//! take on trust (gale#411).
//!
//! The single native atom `mpu_write` arrives as a WIT-typed component
//! import. `gale::mpu_switch` reaches its register store through an
//! `extern "C" fn mpu_write` seam; this module DEFINES that symbol as a
//! forwarder to the import, so the verified code's trusted surface is
//! unchanged and the component's undefined set stays exactly `mpu-write`.
#![no_std]
#[panic_handler]
fn ph(_: &core::panic::PanicInfo) -> ! { loop {} }

use core::alloc::{GlobalAlloc, Layout};
struct NoAlloc;
unsafe impl GlobalAlloc for NoAlloc {
    unsafe fn alloc(&self, _: Layout) -> *mut u8 { core::ptr::null_mut() }
    unsafe fn dealloc(&self, _: *mut u8, _: Layout) {}
}
#[global_allocator]
static ALLOC: NoAlloc = NoAlloc;

wit_bindgen::generate!({ world: "mpu-thin", path: "wit", generate_all });

use gale::mpu_switch as iso;

/// The trusted seam, supplied. `plain/src/mpu_switch.rs` declares
/// `unsafe extern "C" fn mpu_write(rnr, rbar, rasr)` and calls it from
/// `emit_write`, which is `#[verifier::external_body]` and carries no
/// `ensures` — no proof rests on what the store does. Here that symbol is
/// the component's typed import, so the ONE untrusted step is the same one
/// the verification always excluded.
#[no_mangle]
pub extern "C" fn mpu_write(rnr: u32, rbar: u32, rasr: u32) {
    crate::gust::mpu::regs::mpu_write(rnr, rbar, rasr);
}

// One instance owns the table — the exec-provider construction.
static mut TABLE: Option<iso::RegionTable> = None;
fn table() -> &'static mut iso::RegionTable {
    unsafe {
        if TABLE.is_none() { TABLE = Some(iso::RegionTable::new()); }
        TABLE.as_mut().unwrap()
    }
}

struct P;
impl exports::gust::mpu::iso::Guest for P {
    fn size_field(size: u32) -> u32 { iso::size_field(size) }
    // The WIT seam still carries only the privileged axis, so this maps to
    // UNPRIV_SAME — the permission this component could express before
    // gale#410, unchanged. Exposing `unpriv` through WIT is a seam change and
    // belongs on the spar -> WIT path, not a hand edit here.
    fn rasr_for(size: u32, writable: bool) -> u32 {
        iso::rasr_for(size, writable, gale::mpu_switch::UNPRIV_SAME)
    }
    fn try_add_region(part: u32, base: u32, size: u32, writable: bool) -> bool {
        table().try_add_region(part, base, size, writable)
    }
    fn covers_addr(part: u32, addr: u32) -> bool { table().covers_addr(part, addr) }
    fn switch_to_partition(part: u32) { table().switch_to_partition(part) }
}
export!(P);
