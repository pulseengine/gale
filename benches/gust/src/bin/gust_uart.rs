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

extern "C" {
    // dissolved thin-seam UART driver primitives (drivers/uart-thin → synth).
    // No linmem data in the driver → no r11 trampoline needed; call directly.
    fn uart_init(brr: u32);
    fn uart_tx_byte(b: u32);
}

#[entry]
fn main() -> ! {
    // The driver provides the protocol; the APP owns the payload. The test line
    // lives here (normal cortex-m flash), not in the driver — so the driver
    // carries no data segment (0 linmem / 0 SRAM, no placement dependency).
    let msg = b"gust-uart-thin\n";
    unsafe {
        // --- F1 board bring-up (TCB side: SoC clock tree + pin mux, not driver
        // logic) — needed on real STM32F100 silicon; harmless no-ops in Renode's
        // USART model (which doesn't gate on RCC/GPIO). ---
        const RCC_APB2ENR: u32 = 0x4002_1018; // AFIOEN=0, IOPAEN=2, USART1EN=14
        const GPIOA_CRH: u32 = 0x4001_0804; // PA9 config = bits 4..7
        let e = read_volatile(RCC_APB2ENR as *const u32);
        write_volatile(RCC_APB2ENR as *mut u32, e | (1 << 0) | (1 << 2) | (1 << 14));
        let c = read_volatile(GPIOA_CRH as *const u32);
        // PA9 → alternate-function push-pull, 50 MHz (CNF=0b10, MODE=0b11 = 0xB)
        write_volatile(GPIOA_CRH as *mut u32, (c & !(0xF << 4)) | (0xB << 4));

        // 0x45 = 8 MHz HSI (F100 reset clock) / 115200 baud, the driver's USART setup.
        uart_init(0x45);
        for &b in msg {
            uart_tx_byte(b as u32);
        }
    }
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}
