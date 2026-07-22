//! gust-i2c — the thin-seam I2C driver driven bare-metal on gust, with a
//! self-checking Renode content-gate (gust-OS driver breadth close-out).
//!
//! Links ONLY the dissolved i2c-thin driver (mmio bridge, 0 new TCB atoms); the raw
//! USART1 poke is trusted plumbing to report results. Asserts (a) the CR2 FREQ / CCR
//! divisor / TRISE / CR1.PE the DRIVER writes on a RAM-mapped I2C1 window, (b) that a
//! read START issues CR1 = PE|START|ACK, and (c) the Kani-proven ACK-all-but-last
//! property made observable at the register level: over a 3-byte master read the ACK
//! decision is [1,1,0] and CR1.STOP is written EXACTLY on the last byte, never before
//! — deterministic, no dependence on Renode's I2C peripheral model.
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
    fn i2c_configure(base: u32, apb1_mhz: u32, divisor: u32, fast: u32, trise: u32);
    fn i2c_start(base: u32, state: u32, count: u32, read: u32) -> u32;
    fn i2c_addr_ack(state: u32) -> u32;
    fn i2c_step(base: u32, state: u32, out: u32) -> u32;
    fn i2c_ack_byte(state: u32) -> u32;
    fn i2c_is_complete(state: u32) -> u32;
    fn i2c_stop(base: u32, state: u32) -> u32;
}

const I2C1: u32 = 0x4000_5400; // RAM-mapped I2C1 window in the gate .repl
const I2C_CR1: u32 = 0x00;
const I2C_CR2: u32 = 0x04;
const I2C_SR1: u32 = 0x14;
const I2C_CCR: u32 = 0x1C;
const I2C_TRISE: u32 = 0x20;
const I2C_FAULT: u32 = 0xFFFF_FFFF;

const CR1_PE: u32 = 1 << 0;
const CR1_START: u32 = 1 << 8;
const CR1_STOP: u32 = 1 << 9;
const CR1_ACK: u32 = 1 << 10;
const SR1_SEED: u32 = (1 << 0) | (1 << 1) | (1 << 6) | (1 << 7); // SB|ADDR|RXNE|TXE

const APB1_MHZ: u32 = 8;
const DIVISOR: u32 = 0x28;
const TRISE_V: u32 = 9;
const CR1_START_READ: u32 = CR1_PE | CR1_START | CR1_ACK; // 0x501
const CR1_STOP_LAST: u32 = CR1_PE | CR1_STOP; // 0x201

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

        tx(b"i2c-gate begin\n");

        // 1) config: the DRIVER computes CR2 FREQ (APB1 MHz), CCR divisor, TRISE, and
        //    enables PE — all pure bit arithmetic (table-free). Read back the window.
        i2c_configure(I2C1, APB1_MHZ, DIVISOR, 0, TRISE_V);
        let cr2 = read_volatile((I2C1 + I2C_CR2) as *const u32);
        let ccr = read_volatile((I2C1 + I2C_CCR) as *const u32);
        let trise = read_volatile((I2C1 + I2C_TRISE) as *const u32);
        let cr1 = read_volatile((I2C1 + I2C_CR1) as *const u32);
        tx(if cr2 == APB1_MHZ && ccr == DIVISOR && trise == TRISE_V && cr1 == CR1_PE {
            b"i2c-config-ok\n"
        } else {
            b"i2c-config-bad\n"
        });

        // 2) START a 3-byte READ: driver writes CR1 = PE|START|ACK and polls SR1.SB.
        //    Pre-seed SR1 (SB|ADDR|RXNE|TXE) so the polled loops pass on plain memory.
        write_volatile((I2C1 + I2C_SR1) as *mut u32, SR1_SEED);
        let s_addr = i2c_start(I2C1, 0, 3, 1);
        let cr1_start = read_volatile((I2C1 + I2C_CR1) as *const u32);
        tx(if s_addr != I2C_FAULT && cr1_start == CR1_START_READ {
            b"i2c-start-ok\n"
        } else {
            b"i2c-start-bad\n"
        });

        // 3) ACK-all-but-last (Kani-proven p3): address ACKed → Active(rem=3); over the
        //    three bytes the ack decision is [1,1,0], CR1 keeps ACK (no STOP) through
        //    bytes 1..2, and STOP is written EXACTLY on the last byte.
        let s_active = i2c_addr_ack(s_addr);
        let ack1 = i2c_ack_byte(s_active);
        let s1 = i2c_step(I2C1, s_active, 0);
        let cr1_1 = read_volatile((I2C1 + I2C_CR1) as *const u32);
        let ack2 = i2c_ack_byte(s1);
        let s2 = i2c_step(I2C1, s1, 0);
        let cr1_2 = read_volatile((I2C1 + I2C_CR1) as *const u32);
        let ack3 = i2c_ack_byte(s2);
        let s3 = i2c_step(I2C1, s2, 0);
        let cr1_3 = read_volatile((I2C1 + I2C_CR1) as *const u32);
        tx(
            if ack1 == 1
                && ack2 == 1
                && ack3 == 0
                && cr1_1 == CR1_START_READ
                && cr1_2 == CR1_START_READ
                && cr1_3 == CR1_STOP_LAST
            {
                b"i2c-ack-rule-ok\n"
            } else {
                b"i2c-ack-rule-bad\n"
            },
        );

        // 4) completeness: Complete after the last byte, not mid-run.
        let done = i2c_is_complete(s3);
        let mid = i2c_is_complete(s_active);
        tx(if done == 1 && mid == 0 {
            b"i2c-complete-ok\n"
        } else {
            b"i2c-complete-bad\n"
        });

        // 5) exclusive bus + phase gating (Kani p1/p4): a second START onto the
        //    in-flight transaction faults (BusBusy), and `addr_ack` from a
        //    non-Addressing phase faults (WrongPhase) — both mmio-free reject paths.
        //    (`i2c_step` off-Active is not called: the dissolved step polls SR1
        //    unconditionally and would busy-wait; its phase gate is Kani-proven at
        //    source, see RESULTS.md.)
        let busy = i2c_start(I2C1, s_active, 1, 0);
        let ack_offphase = i2c_addr_ack(s3); // s3 is Complete, not Addressing
        tx(if busy == I2C_FAULT && ack_offphase == I2C_FAULT {
            b"i2c-fault-ok\n"
        } else {
            b"i2c-fault-bad\n"
        });

        // 6) STOP frees the bus back to Idle from any state (Kani p5), writing CR1.STOP.
        let idle = i2c_stop(I2C1, s_active);
        let cr1_stop = read_volatile((I2C1 + I2C_CR1) as *const u32);
        tx(if idle == 0 && cr1_stop == CR1_STOP_LAST {
            b"i2c-stop-ok\n"
        } else {
            b"i2c-stop-bad\n"
        });

        tx(b"i2c-gate done\n");
    }
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}
