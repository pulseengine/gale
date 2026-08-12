#![no_std]
//! Seam stub for the MC/DC evidence run: the one native atom is a no-op.
//! The same substitution the Kani harness makes — `mpu_write`'s barrier-pairing
//! contract is trusted platform code and cannot be linked under wasmtime. What is
//! under test is the region-programming POLICY, never the register write.
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

wit_bindgen::generate!({ path: "wit", world: "regs-stub" });

struct S;
impl exports::gust::mpu::regs::Guest for S {
    fn mpu_write(_rnr: u32, _rbar: u32, _rasr: u32) {}
}
export!(S);
