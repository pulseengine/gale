//! gust-gpio — the thin-seam GPIO driver driven bare-metal on the gust stack, with
//! a self-checking Renode content-gate (gust-OS v0.3.0 driver breadth).
//!
//! The dissolved GPIO driver (`drivers/gpio-thin`, the entire STM32F1 GPIO protocol
//! in verified wasm → synth → native) is the ONLY dissolved object linked here (so
//! it is unambiguously the thing under test); this file is the whole trusted side —
//! the SAME `gust:hal` MMIO bridge (no new TCB atom) plus a raw USART1 poke used only
//! to *report* results to the robot tester. (We do not co-link the UART driver: two
//! synth modules collide on their internal `func_N` symbols — a real multi-driver
//! composition constraint tracked under REQ-DRV-BREADTH-001; the raw poke sidesteps
//! it and keeps the GPIO driver isolated as the object under test.) The assertions
//! are about the GPIO driver's register effects, which Renode's peripheral model
//! faithfully stores (independent of pin electricals).
//!
//! Boot: STM32F100 (Cortex-M3) in Renode; drives PC8 via the GPIO driver, checks the
//! resulting CRH/ODR bits, and TXes `gpio-*-ok`/`-bad` lines over USART1.
#![no_std]
#![no_main]
use core::ptr::{read_volatile, write_volatile};
use cortex_m_rt::entry;
use cortex_m_semihosting::debug;
use panic_halt as _;

// ---- gust:hal MMIO bridge — the entire trusted surface (shared with uart-thin) ----
#[no_mangle]
pub extern "C" fn mmio_read32(addr: u32) -> u32 {
    unsafe { read_volatile(addr as *const u32) }
}
#[no_mangle]
pub extern "C" fn mmio_write32(addr: u32, val: u32) {
    unsafe { write_volatile(addr as *mut u32, val) }
}
/// irq.poll — required by the linked uart driver; unused here (GPIO needs no irq).
#[no_mangle]
pub extern "C" fn irq_poll(_line: u32) -> u32 {
    0
}

extern "C" {
    // dissolved thin-seam GPIO driver (drivers/gpio-thin → synth). Scalar ABI.
    fn gpio_configure(port_base: u32, pin: u32, mode_idx: u32);
    fn gpio_set(port_base: u32, pin: u32);
    fn gpio_clear(port_base: u32, pin: u32);
}

const GPIOC: u32 = 0x4001_1000; // STM32F1 GPIO port C base (RAM-mapped in the gate .repl)
const CRH: u32 = 0x04; // config, pins 8..=15
const BSRR: u32 = 0x10; // bit set (0..15) / reset (16..31)
const MODE_OUT_PP50: u32 = 4; // gpio-thin mode_from_idx(4) = OutPushPull50 (nibble 0x3)
const PIN: u32 = 8; // PC8 → CRH bits 0..=3

// The gate asserts the register values the DRIVER writes (its verified logic) on a
// RAM-mapped GPIOC region — deterministic, no dependence on Renode's GPIO peripheral
// model (which need not exist / faithfully emulate F1 BSRR→ODR). The BSRR→pin
// electrical behaviour is the chip's job, verified separately on real silicon.

// Raw USART1 report channel (trusted TCB plumbing, not under test).
const USART1: u32 = 0x4001_3800;
const USART_SR: u32 = 0x00;
const USART_DR: u32 = 0x04;
const USART_BRR: u32 = 0x08;
const USART_CR1: u32 = 0x0C;
const TXE: u32 = 1 << 7;

fn tx(s: &[u8]) {
    for &b in s {
        unsafe {
            while read_volatile((USART1 + USART_SR) as *const u32) & TXE == 0 {}
            write_volatile((USART1 + USART_DR) as *mut u32, (b as u32) & 0xFF);
        }
    }
}

#[entry]
fn main() -> ! {
    unsafe {
        // TCB board bring-up: enable GPIOA (PA9=USART1 TX), GPIOC, AFIO, USART1.
        const RCC_APB2ENR: u32 = 0x4002_1018;
        let e = read_volatile(RCC_APB2ENR as *const u32);
        write_volatile(
            RCC_APB2ENR as *mut u32,
            e | (1 << 0) | (1 << 2) | (1 << 4) | (1 << 14), // AFIO|IOPA|IOPC|USART1
        );
        // PA9 → AF push-pull 50MHz for USART1 TX (same as gust_uart).
        const GPIOA_CRH: u32 = 0x4001_0804;
        let c = read_volatile(GPIOA_CRH as *const u32);
        write_volatile(GPIOA_CRH as *mut u32, (c & !(0xF << 4)) | (0xB << 4));
        // USART1: 8MHz/115200 baud, enable + TX (raw TCB setup).
        write_volatile((USART1 + USART_BRR) as *mut u32, 0x45);
        write_volatile((USART1 + USART_CR1) as *mut u32, (1 << 13) | (1 << 3)); // UE | TE

        tx(b"gpio-gate begin\n");

        // 1) configure PC8 as output push-pull 50MHz via the DRIVER; check CRH nibble.
        gpio_configure(GPIOC, PIN, MODE_OUT_PP50);
        let crh = read_volatile((GPIOC + CRH) as *const u32);
        tx(if (crh & 0xF) == 0x3 {
            b"gpio-cfg-ok\n"
        } else {
            b"gpio-cfg-bad\n"
        });

        // 2) set PC8 via the DRIVER; it must write BSRR = 1<<8 (atomic set).
        gpio_set(GPIOC, PIN);
        let bsrr_set = read_volatile((GPIOC + BSRR) as *const u32);
        tx(if bsrr_set == (1 << 8) {
            b"gpio-set-ok\n"
        } else {
            b"gpio-set-bad\n"
        });

        // 3) clear PC8 via the DRIVER; it must write BSRR = 1<<(8+16) (atomic reset).
        gpio_clear(GPIOC, PIN);
        let bsrr_clr = read_volatile((GPIOC + BSRR) as *const u32);
        tx(if bsrr_clr == (1 << 24) {
            b"gpio-clear-ok\n"
        } else {
            b"gpio-clear-bad\n"
        });

        tx(b"gpio-gate done\n");
    }
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}
