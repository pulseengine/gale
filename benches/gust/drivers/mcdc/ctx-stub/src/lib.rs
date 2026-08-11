#![no_std]
//! Seam stub for the MC/DC evidence run. The three native atoms return success,
//! exactly as the Kani harness substitutes them — the FSM's own policy is what is
//! under test here, not the native context save/restore.
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

wit_bindgen::generate!({ path: "wit", world: "ctx-stub" });

struct S;
impl exports::gust::switch::ctx::Guest for S {
    fn ctx_save(_part: u32) -> u32 { 0 }
    fn region_swap(_part: u32) -> u32 { 0 }
    fn ctx_resume(_part: u32) -> u32 { 0 }
}
export!(S);
