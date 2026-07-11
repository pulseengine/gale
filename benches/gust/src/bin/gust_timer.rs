//! gust-timer — the thin-seam hardware-timer driver driven bare-metal on gust, with a
//! self-checking Renode content-gate (gust-OS v0.3.0 driver breadth).
//!
//! Links ONLY the dissolved timer-thin driver (mmio bridge, 0 new TCB atoms); the raw
//! USART1 poke is trusted plumbing to report results. Asserts the driver's register
//! writes (PSC/ARR/CR1) on a RAM-mapped TIM window AND its wrap-safe deadline math
//! (Kani-proven) — deterministic, no dependence on Renode's timer peripheral model.
#![no_std]
#![no_main]
use core::ptr::{read_volatile, write_volatile};
use cortex_m_rt::entry;
use cortex_m_semihosting::debug;
use panic_halt as _;

#[no_mangle]
pub extern "C" fn mmio_read32(addr: u32) -> u32 {
    unsafe { read_volatile(addr as *const u32) }
}
#[no_mangle]
pub extern "C" fn mmio_write32(addr: u32, val: u32) {
    unsafe { write_volatile(addr as *mut u32, val) }
}

extern "C" {
    fn timer_init(base: u32, psc: u32, arr: u32);
    fn timer_deadline(now: u32, ticks: u32) -> u32;
    fn timer_elapsed(now: u32, deadline: u32) -> u32;
}

const TIM: u32 = 0x4000_0000; // TIM2 base (RAM-mapped in the gate .repl)
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
        // enable GPIOA(PA9 TX), AFIO, USART1; PA9 → AF push-pull; USART1 8MHz/115200.
        const RCC_APB2ENR: u32 = 0x4002_1018;
        let e = read_volatile(RCC_APB2ENR as *const u32);
        write_volatile(RCC_APB2ENR as *mut u32, e | (1 << 0) | (1 << 2) | (1 << 14));
        const GPIOA_CRH: u32 = 0x4001_0804;
        let c = read_volatile(GPIOA_CRH as *const u32);
        write_volatile(GPIOA_CRH as *mut u32, (c & !(0xF << 4)) | (0xB << 4));
        write_volatile((USART1 + USART_BRR) as *mut u32, 0x45);
        write_volatile((USART1 + USART_CR1) as *mut u32, (1 << 13) | (1 << 3));

        tx(b"timer-gate begin\n");

        // 1) init writes PSC/ARR/CR1(CEN) via the DRIVER.
        timer_init(TIM, 0x1234, 0xABCD);
        let psc = read_volatile((TIM + 0x28) as *const u32);
        let arr = read_volatile((TIM + 0x2C) as *const u32);
        let cr1 = read_volatile((TIM + 0x00) as *const u32);
        tx(if psc == 0x1234 && arr == 0xABCD && (cr1 & 1) != 0 {
            b"timer-init-ok\n"
        } else {
            b"timer-init-bad\n"
        });

        // 2) deadline math (Kani-proven) — a future deadline hasn't elapsed, an at/past one has.
        let d = timer_deadline(100, 50);
        tx(if d == 150 && timer_elapsed(149, d) == 0 && timer_elapsed(150, d) == 1 {
            b"timer-deadline-ok\n"
        } else {
            b"timer-deadline-bad\n"
        });

        // 3) WRAP-SAFE: a deadline past u32::MAX, reached by a wrapped 'now'.
        let dw = timer_deadline(0xFFFF_FFF0, 0x20); // = 0x10
        tx(if dw == 0x10 && timer_elapsed(0x0F, dw) == 0 && timer_elapsed(0x10, dw) == 1 {
            b"timer-wrap-ok\n"
        } else {
            b"timer-wrap-bad\n"
        });

        tx(b"timer-gate done\n");
    }
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}
