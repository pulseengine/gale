//! gust-breadth — the 4-driver BREADTH node (REQ-DRV-BREADTH-001) driven bare-metal
//! on gust, with a self-checking Renode content-gate. Links ONE dissolved object
//! (drivers/breadth/breadth-cm3.o) that is gpio+timer+spi+uart — four verified-wasm
//! gust:hal COMPONENTS, wac/meld-fused into a single relocatable module (0 SRAM, no
//! func_N collision). The whole TCB is the 3-atom bridge below: read32 / write32 /
//! poll. Asserts each driver's register effects / FSM on a real STM32 model and
//! emits breadth-*-ok over USART1 — the last line is TX'd BY the dissolved uart
//! driver itself, proving all four are live end-to-end from the one fused object.
#![no_std]
#![no_main]
use core::ptr::{read_volatile, write_volatile};
use cortex_m_rt::entry;
use cortex_m_semihosting::debug;
use panic_halt as _;

// The gust:hal capability bridge the fused .o imports (gust:hal/mmio.{read32,
// write32} + gust:hal/irq.poll) — the entire trusted-native surface, 3 atoms.
#[no_mangle]
pub extern "C" fn read32(addr: u32) -> u32 { unsafe { read_volatile(addr as *const u32) } }
#[no_mangle]
pub extern "C" fn write32(addr: u32, val: u32) { unsafe { write_volatile(addr as *mut u32, val) } }
#[no_mangle]
pub extern "C" fn poll(_line: u32) -> u32 { 1 }

extern "C" {
    fn gpio_configure(base: u32, pin: u32, mode_idx: u32);
    fn timer_init(base: u32, psc: u32, arr: u32);
    fn timer_deadline(now: u32, ticks: u32) -> u32;
    fn timer_elapsed(now: u32, deadline: u32) -> u32;
    fn spi_configure(base: u32, mode: u32, br_idx: u32);
    fn spi_begin(state: u32, count: u32) -> u32;
    fn spi_step(state: u32) -> u32;
    fn spi_is_complete(state: u32) -> u32;
    fn uart_tx_byte(b: u32);
}

const GPIOC: u32 = 0x4001_1000; // RAM-mapped window in the gate .repl
const TIM2: u32 = 0x4000_0000; // RAM-mapped window
const SPI1: u32 = 0x4001_3000; // RAM-mapped window
const USART1: u32 = 0x4001_3800; // real STM32_UART model
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
        const RCC_APB2ENR: u32 = 0x4002_1018;
        let e = read_volatile(RCC_APB2ENR as *const u32);
        write_volatile(RCC_APB2ENR as *mut u32, e | (1 << 0) | (1 << 2) | (1 << 14));
        const GPIOA_CRH: u32 = 0x4001_0804;
        let c = read_volatile(GPIOA_CRH as *const u32);
        write_volatile(GPIOA_CRH as *mut u32, (c & !(0xF << 4)) | (0xB << 4));
        write_volatile((USART1 + USART_BRR) as *mut u32, 0x45);
        write_volatile((USART1 + USART_CR1) as *mut u32, (1 << 13) | (1 << 3));

        tx(b"breadth-gate begin\n");

        // GPIO — configure PC8 as output (mode idx 4 → CRH nibble 0x3).
        gpio_configure(GPIOC, 8, 4);
        let crh = read_volatile((GPIOC + 0x04) as *const u32);
        tx(if crh & 0xF == 0x3 { b"breadth-gpio-ok\n" } else { b"breadth-gpio-bad\n" });

        // TIMER — init writes PSC/ARR/CR1(CEN); wrap-safe deadline math (Kani-proven).
        timer_init(TIM2, 0x1234, 0xABCD);
        let psc = read_volatile((TIM2 + 0x28) as *const u32);
        let arr = read_volatile((TIM2 + 0x2C) as *const u32);
        let cr1 = read_volatile((TIM2 + 0x00) as *const u32);
        let d = timer_deadline(100, 50);
        tx(if psc == 0x1234 && arr == 0xABCD && cr1 & 1 != 0
            && d == 150 && timer_elapsed(149, d) == 0 && timer_elapsed(150, d) == 1
        { b"breadth-timer-ok\n" } else { b"breadth-timer-bad\n" });

        // SPI — configure (mode3/br2 → CR1=0x357) + transfer FSM begin→step×3→complete.
        spi_configure(SPI1, 3, 2);
        let scr1 = read_volatile((SPI1 + 0x00) as *const u32);
        let s3 = spi_step(spi_step(spi_step(spi_begin(0, 3))));
        tx(if scr1 == 0x357 && spi_is_complete(s3) == 1 { b"breadth-spi-ok\n" } else { b"breadth-spi-bad\n" });

        // UART — the dissolved uart driver TXes the proof line itself over the real
        // USART1 (same peripheral this gate reports on): all four drivers live from
        // the one fused object.
        for &b in b"breadth-uart-ok\n" { uart_tx_byte((b as u32) & 0xFF); }

        tx(b"breadth-gate done\n");
    }
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}
