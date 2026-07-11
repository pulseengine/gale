//! gust-spi — the thin-seam SPI driver (first RTIO iodev) driven bare-metal on gust,
//! with a self-checking Renode content-gate (gust-OS v0.3.0 driver breadth).
//!
//! Links ONLY the dissolved spi-thin driver (mmio bridge, 0 new TCB atoms); the raw
//! USART1 poke is trusted plumbing to report results. Asserts (a) the CR1 mode/baud
//! value the DRIVER writes on a RAM-mapped SPI1 window, (b) the full-duplex byte
//! shift over a pre-seeded SR/DR window, and (c) the Kani-proven transfer FSM
//! (exclusive bus + no-lost-byte, SQE→CQE) — deterministic, no dependence on
//! Renode's SPI peripheral model.
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
    fn spi_configure(base: u32, mode: u32, br_idx: u32);
    fn spi_xfer_byte(base: u32, out: u32) -> u32;
    fn spi_begin(state: u32, count: u32) -> u32;
    fn spi_step(state: u32) -> u32;
    fn spi_is_complete(state: u32) -> u32;
    fn spi_abort(state: u32) -> u32;
}

const SPI1: u32 = 0x4001_3000; // RAM-mapped SPI1 window in the gate .repl
const SPI_CR1: u32 = 0x00;
const SPI_SR: u32 = 0x08;
const SPI_DR: u32 = 0x0C;
const SPI_FAULT: u32 = 0xFFFF_FFFF;

// CR1 for (mode=3, br_idx=2): SPE|MSTR|SSM|SSI | (2<<3) | (mode&3)
//   = 0x40 | 0x04 | 0x200 | 0x100 | 0x10 | 0x03 = 0x357
const CR1_EXPECT: u32 = 0x357;

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

        tx(b"spi-gate begin\n");

        // 1) config: the DRIVER computes CR1 (master, SW-NSS, baud/2^3, mode 3) and
        //    writes it in one store. Read it back from the RAM-mapped CR1.
        spi_configure(SPI1, 3, 2);
        let cr1 = read_volatile((SPI1 + SPI_CR1) as *const u32);
        tx(if cr1 == CR1_EXPECT {
            b"spi-config-ok\n"
        } else {
            b"spi-config-bad\n"
        });

        // 2) full-duplex byte: pre-seed SR (TXE|RXNE set) so the polled loops pass,
        //    then the driver writes DR and reads it back (loopback on the RAM window).
        write_volatile((SPI1 + SPI_SR) as *mut u32, 0x3); // TXE(1)|RXNE(0)
        let rx = spi_xfer_byte(SPI1, 0xA5);
        let dr = read_volatile((SPI1 + SPI_DR) as *const u32) & 0xFF;
        tx(if rx == 0xA5 && dr == 0xA5 {
            b"spi-xfer-ok\n"
        } else {
            b"spi-xfer-bad\n"
        });

        // 3) transfer FSM (Kani-proven, SQE→CQE): submit 3 bytes, a second submit
        //    onto the busy bus faults (exclusive bus), three steps reach Complete
        //    (no lost byte), and abort frees the bus back to Idle.
        let s0 = spi_begin(0, 3); // Idle(0) + 3 → Active,rem=3
        let busy = spi_begin(s0, 2); // bus busy → fault
        let s1 = spi_step(s0);
        let s2 = spi_step(s1);
        let s3 = spi_step(s2); // last byte → Complete
        let done = spi_is_complete(s3);
        let mid = spi_is_complete(s0); // still Active → not complete
        let idle = spi_abort(s3); // back to Idle(0)
        tx(if busy == SPI_FAULT && done == 1 && mid == 0 && idle == 0 {
            b"spi-fsm-ok\n"
        } else {
            b"spi-fsm-bad\n"
        });

        tx(b"spi-gate done\n");
    }
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}
