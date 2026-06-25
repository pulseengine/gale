//! gust-uart — the thin-seam UART driver driven bare-metal on the gust stack.
//!
//! The dissolved driver (`drivers/uart-thin`, the ENTIRE STM32 USART protocol in
//! verified wasm → synth → native) is linked here and called; this file is the
//! whole **trusted** side: a ~10-line `gust:hal` THIN bridge (raw MMIO + an IRQ
//! flag) + the boot shim. Nothing peripheral-specific is trusted — the driver
//! owns the registers, the bridge only pokes them.
//!
//! Boot: STM32F100 (Cortex-M3) in Renode with usart1 = UART.STM32_UART; the
//! driver TXes "gust-uart-thin\n" over USART1, asserted by a terminal tester.
#![no_std]
#![no_main]
use core::ptr::{read_volatile, write_volatile};
use cortex_m_rt::entry;
use cortex_m_semihosting::debug;
use panic_halt as _;

// ---- gust:hal THIN bridge — the entire trusted surface for the UART driver ----
#[no_mangle]
pub extern "C" fn mmio_read32(addr: u32) -> u32 {
    unsafe { read_volatile(addr as *const u32) }
}
#[no_mangle]
pub extern "C" fn mmio_write32(addr: u32, val: u32) {
    unsafe { write_volatile(addr as *mut u32, val) }
}
/// irq.poll — would be set by the USART RX ISR; 0 here (TX smoke test, no RX).
#[no_mangle]
pub extern "C" fn irq_poll(_line: u32) -> u32 {
    0
}

// The dissolved driver (synth --native-pointer-abi) addresses its string data
// relative to the wasm linmem base, which the ABI pins to r11 == 0. main() does
// not zero r11, so calls must go through this trampoline (same pattern as
// gust_control). driver_step_body is the objcopy-renamed synth export.
core::arch::global_asm!(
    ".section .text.driver_step",
    ".global driver_step",
    ".thumb_func",
    "driver_step:",
    "    push  {{r11, lr}}",
    "    mov.w r11, #0",
    "    bl    driver_step_body",
    "    pop   {{r11, pc}}",
);

extern "C" {
    // the dissolved thin-seam UART driver via the r11=0 trampoline above.
    fn driver_step() -> u32;
}

#[entry]
fn main() -> ! {
    // One driver step: it inits USART1 and TXes the known line over MMIO.
    let _ = unsafe { driver_step() };
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}
