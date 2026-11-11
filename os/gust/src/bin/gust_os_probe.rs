//! gust-os-probe — LOCAL qemu-semihosting liveness probe of the gust:os v0.4.0
//! STEP-1 node (drivers/os-node/os-time-cm3.o): an app that imports ONLY gust:os/time,
//! wac-plugged with a time provider (backed by gust:hal/mmio), meld-fused + dissolved
//! to one 0-SRAM object exporting `run` and importing only `read32`. Proves the
//! syscall-seam compose is functionally live end-to-end: we provide the read32 TCB
//! atom, call the app's `run`, and check its OS-time logic returns the ok code.
#![no_std]
#![no_main]
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use panic_halt as _;

// The whole TCB the gust:os/time node needs: one mmio read atom (the timer CNT).
#[no_mangle]
pub extern "C" fn read32(_addr: u32) -> u32 { 1000 }

// THE OBJECT IS NOT 0-SRAM. It reads and writes two bytes of wasm linear memory at
// 0x0010_0008 and 0x0010_000C (wasm-ld's data base, 0x10_0000, + 8/12), addressed as
// [r11 + 0x10_0008]. Nothing set r11, so on qemu those were ABSOLUTE addresses in
// unmapped space, which lm3s6965evb reads as zero and silently drops writes to — the
// probe passed. On every real part it is a precise BusFault (G474: CFSR=0x8200,
// BFAR=0x0010_0008, stacked PC inside `run`), and the probe hung in HardFault with
// no output (gale#397).
//
// So the embedder does what the object needs: back that data region with real SRAM.
// OS_TIME_DATA stands in for wasm address 0x10_0000..0x10_0010, zero-initialised as
// qemu's reads were, and `run` is entered with r11 = &OS_TIME_DATA - 0x10_0000.
#[no_mangle]
static mut OS_TIME_DATA: [u8; 16] = [0; 16];

core::arch::global_asm!(
    ".section .text.run_time",
    ".global run_time",
    ".thumb_func",
    "run_time:",
    "    push  {{r11, lr}}",
    "    ldr   r11, =(OS_TIME_DATA - 0x100000)",
    "    bl    run",
    "    pop   {{r11, pc}}",
    "    .ltorg",
);

extern "C" { fn run_time() -> u32; }

#[entry]
fn main() -> ! {
    let r = unsafe { run_time() };
    // The window must have been USED, or this probe is back to proving nothing about
    // the object's memory: `run` sets its guard byte, so at least one byte is non-zero.
    let window = unsafe { core::ptr::read_volatile(core::ptr::addr_of!(OS_TIME_DATA)) };
    let used = window.iter().any(|b| *b != 0);
    if r == 0xA && used {
        hprintln!("gust-os-probe OK: app ran on gust:os/time (run()={:#x}, data window {:02x?})", r, &window[8..16]);
        debug::exit(debug::EXIT_SUCCESS);
    } else {
        hprintln!("gust-os-probe FAIL: run()={:#x} (want 0xA), data window used={}", r, used);
        debug::exit(debug::EXIT_FAILURE);
    }
    loop {}
}
