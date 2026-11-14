//! gust-sen66 — the air-sensor stack on Renode's model of the STM32 I2C v2 block
//! (gale#511). Links the two dissolved drivers and nothing else of the stack:
//!
//! * drivers/sen66-thin  (exports gust:sen66/air; imports gust:bus/i2c-master)
//! * drivers/i2c-v2-thin (exports gust:bus/i2c-master and gust:i2c-v2/setup;
//!   imports gust:hal/mmio, which gust_armv7m supplies as read32/write32)
//!
//! The three `begin`/`xfer`/`end` shims below are the composition step: they bind the
//! sensor driver's gust:bus imports to the bus driver's exports, which is what a meld
//! composition does. They are plumbing in this test binary, not driver logic.
//!
//! The firmware prints raw results only. renode-test/gust_sen66.robot queues the
//! sensor model's answers (sen66_model.cs), computes the expected packed values
//! itself, and checks each line, plus the model's own counters.
#![no_std]
#![no_main]
use core::ptr::{read_volatile, write_volatile};
use cortex_m_rt::entry;
use cortex_m_semihosting::debug;
use panic_halt as _;

// gust:hal/mmio bridge (read32/write32) for the bus driver.
use gust_armv7m as _;

extern "C" {
    #[link_name = "gust:i2c-v2/setup@0.1.0#configure"]
    fn i2cv2_configure(timingr: u32) -> u32;
    #[link_name = "gust:bus/i2c-master@0.1.0#begin"]
    fn i2cv2_begin(addr7: u32, count: u32, read: u32) -> u32;
    #[link_name = "gust:bus/i2c-master@0.1.0#xfer"]
    fn i2cv2_xfer(state: u32, out: u32) -> u32;
    #[link_name = "gust:bus/i2c-master@0.1.0#end"]
    fn i2cv2_end(state: u32) -> u32;

    #[link_name = "gust:sen66/air@0.1.0#start"]
    fn sen66_start() -> u32;
    #[link_name = "gust:sen66/air@0.1.0#read"]
    fn sen66_read() -> u64;
    #[link_name = "gust:sen66/air@0.1.0#stop"]
    fn sen66_stop() -> u32;
}

// Composition shims: sen66-thin's undefined `begin`/`xfer`/`end` (its gust:bus
// imports) resolve to these, which call i2c-v2-thin's exports.
#[export_name = "begin"]
pub extern "C" fn bus_begin(addr7: u32, count: u32, read: u32) -> u32 {
    unsafe { i2cv2_begin(addr7, count, read) }
}
#[export_name = "xfer"]
pub extern "C" fn bus_xfer(state: u32, out: u32) -> u32 {
    unsafe { i2cv2_xfer(state, out) }
}
#[export_name = "end"]
pub extern "C" fn bus_end(state: u32) -> u32 {
    unsafe { i2cv2_end(state) }
}

// A TIMINGR word with the reserved bits 24..=27 clear. Renode's model does not time
// the bus, so the value only has to pass the driver's reserved-bit check.
const TIMINGR: u32 = 0x0030_3D5B;
const READS: u32 = 6;

const USART1: u32 = 0x4001_3800;
const USART_BRR: u32 = 0x08;
const USART_CR1: u32 = 0x0C;

#[path = "../../../../targets/generated/gust_target_stm32f100.rs"]
#[allow(dead_code)]
mod target;
fn tx(s: &[u8]) {
    gust_baremetal::console::tx(target::USART1_SR, target::USART1_DR, s);
}

fn hex(v: u64, digits: u32) {
    let mut buf = [0u8; 16];
    let mut i = 0;
    while i < digits {
        let nib = ((v >> (4 * (digits - 1 - i))) & 0xF) as u8;
        buf[i as usize] = if nib < 10 { b'0' + nib } else { b'a' + nib - 10 };
        i += 1;
    }
    tx(&buf[..digits as usize]);
}

fn line_u32(label: &[u8], v: u32) {
    tx(b"sen66-gate ");
    tx(label);
    tx(b"=0x");
    hex(v as u64, 8);
    tx(b"\n");
}

#[entry]
fn main() -> ! {
    unsafe {
        // USART1 for the result lines (the Renode STM32_UART model needs only UE|TE).
        write_volatile((USART1 + USART_BRR) as *mut u32, 0x45);
        write_volatile((USART1 + USART_CR1) as *mut u32, (1 << 13) | (1 << 3));
        let _ = read_volatile((USART1 + USART_CR1) as *const u32);
    }
    tx(b"sen66-gate begin\n");

    line_u32(b"configure", unsafe { i2cv2_configure(TIMINGR) });
    line_u32(b"start", unsafe { sen66_start() });
    let mut n = 1;
    while n <= READS {
        let v = unsafe { sen66_read() };
        tx(b"sen66-gate read");
        hex(n as u64, 1);
        tx(b"=0x");
        hex(v, 16);
        tx(b"\n");
        n += 1;
    }
    line_u32(b"stop", unsafe { sen66_stop() });

    tx(b"sen66-gate done\n");
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}
