//! gust-i2c-probe — the LOCAL qemu-semihosting probe of the DISSOLVED i2c-thin .o,
//! run BEFORE the Renode content-gate to catch table/linmem no-op bugs fast.
//!
//! Points the dissolved I2C driver at a plain `[u32; 16]` RAM window (real mapped
//! SRAM on lm3s6965evb) and checks the exact register effects + the transaction FSM
//! via semihosting — so a dissolved primitive that silently no-ops (e.g. a `.rodata`
//! linmem lookup that reads 0 under `--relocatable`) fails HERE, on `cargo run`, not
//! three CI minutes later in Renode. Also DEMONSTRATES the driver's distinctive
//! safety property, **ACK-all-but-last**: on a master read of N bytes the master ACKs
//! bytes 1..N−1 (CR1.ACK stays set, no STOP) and NACKs the last (CR1.STOP written
//! exactly on the final byte, and only then) — the Kani-proven `ack_byte` rule made
//! observable at the register level.
#![no_std]
#![no_main]
use core::ptr::{addr_of_mut, read_volatile, write_volatile};
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use panic_halt as _;

// The mmio seam the dissolved driver imports — here it reads/writes the RAM window.
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

// Fake STM32F1 I2C register window in RAM: word i = byte offset i*4.
// CR1=0x00(idx0), CR2=0x04(idx1), DR=0x10(idx4), SR1=0x14(idx5), CCR=0x1C(idx7),
// TRISE=0x20(idx8).
static mut REG: [u32; 16] = [0; 16];

const I2C_FAULT: u32 = 0xFFFF_FFFF;

// CR1 bits the driver writes.
const CR1_PE: u32 = 1 << 0;
const CR1_START: u32 = 1 << 8;
const CR1_STOP: u32 = 1 << 9;
const CR1_ACK: u32 = 1 << 10;

// SR1 flags the driver polls (SB|ADDR|RXNE|TXE) — pre-seed so the busy-waits pass.
const SR1_SEED: u32 = (1 << 0) | (1 << 1) | (1 << 6) | (1 << 7); // 0xC3

// Config inputs and their pure-arithmetic register images (table-free driver).
const APB1_MHZ: u32 = 8; // CR2 FREQ = 8 & 0x3F = 8
const DIVISOR: u32 = 0x28; // CCR = 0x28 (fast=0, no CCR_FS)
const TRISE_V: u32 = 9;
const CR1_START_READ: u32 = CR1_PE | CR1_START | CR1_ACK; // 0x501 — read issues ACK
const CR1_STOP_LAST: u32 = CR1_PE | CR1_STOP; // 0x201 — NACK/STOP on the last byte

#[entry]
fn main() -> ! {
    let base = addr_of_mut!(REG) as u32;
    let mut ok = true;

    // 1) configure: driver must WRITE CR2 FREQ, CCR divisor, TRISE, and enable PE.
    //    A dissolved no-op leaves the window 0.
    unsafe { i2c_configure(base, APB1_MHZ, DIVISOR, 0, TRISE_V) };
    let cr2 = unsafe { read_volatile(addr_of_mut!(REG[1])) };
    let ccr = unsafe { read_volatile(addr_of_mut!(REG[7])) };
    let trise = unsafe { read_volatile(addr_of_mut!(REG[8])) };
    let cr1_cfg = unsafe { read_volatile(addr_of_mut!(REG[0])) };
    if cr2 != APB1_MHZ || ccr != DIVISOR || trise != TRISE_V || cr1_cfg != CR1_PE {
        hprintln!(
            "i2c-config FAIL: CR2={:#x} CCR={:#x} TRISE={:#x} CR1={:#x}",
            cr2, ccr, trise, cr1_cfg
        );
        ok = false;
    } else {
        hprintln!("i2c-config ok: CR2={:#x} CCR={:#x} TRISE={:#x} CR1(PE)={:#x}", cr2, ccr, trise, cr1_cfg);
    }

    // 2) START a 3-byte READ: driver writes CR1 = PE|START|ACK (a read arms ACK) and
    //    polls SR1.SB. Seed SR1 first so the busy-wait terminates.
    unsafe { write_volatile(addr_of_mut!(REG[5]), SR1_SEED) };
    let s_addr = unsafe { i2c_start(base, 0, 3, 1) };
    let cr1_start = unsafe { read_volatile(addr_of_mut!(REG[0])) };
    if s_addr == I2C_FAULT || cr1_start != CR1_START_READ {
        hprintln!("i2c-start FAIL: s={:#x} CR1={:#x} want {:#x}", s_addr, cr1_start, CR1_START_READ);
        ok = false;
    } else {
        hprintln!("i2c-start ok: CR1={:#x} (PE|START|ACK on read)", cr1_start);
    }

    // address ACKed: Addressing -> Active (rem=3).
    let s_active = unsafe { i2c_addr_ack(s_addr) };

    // 3) ACK-all-but-last (the distinctive property, Kani p3): over the 3-byte read the
    //    ack decision is [1,1,0], CR1 keeps ACK (no STOP) through bytes 1..2, and STOP
    //    is written EXACTLY on the last byte — never before.
    let ack1 = unsafe { i2c_ack_byte(s_active) }; // rem=3 -> 1
    let s1 = unsafe { i2c_step(base, s_active, 0) };
    let cr1_after1 = unsafe { read_volatile(addr_of_mut!(REG[0])) };

    let ack2 = unsafe { i2c_ack_byte(s1) }; // rem=2 -> 1
    let s2 = unsafe { i2c_step(base, s1, 0) };
    let cr1_after2 = unsafe { read_volatile(addr_of_mut!(REG[0])) };

    let ack3 = unsafe { i2c_ack_byte(s2) }; // rem=1 (last) -> 0
    let s3 = unsafe { i2c_step(base, s2, 0) };
    let cr1_after3 = unsafe { read_volatile(addr_of_mut!(REG[0])) };

    if ack1 != 1
        || ack2 != 1
        || ack3 != 0
        || cr1_after1 != CR1_START_READ // ACK still set, STOP NOT written yet
        || cr1_after2 != CR1_START_READ
        || cr1_after3 != CR1_STOP_LAST // STOP written exactly on the last byte
    {
        hprintln!(
            "i2c-ack-rule FAIL: ack=[{},{},{}] CR1=[{:#x},{:#x},{:#x}]",
            ack1, ack2, ack3, cr1_after1, cr1_after2, cr1_after3
        );
        ok = false;
    } else {
        hprintln!("i2c-ack-rule ok: ack=[1,1,0], STOP written only on the last byte (CR1 {:#x}->{:#x})", CR1_START_READ, CR1_STOP_LAST);
    }

    // 4) completeness: after the last step the transaction is Complete; mid-run it is not.
    let done = unsafe { i2c_is_complete(s3) };
    let mid = unsafe { i2c_is_complete(s_active) };
    if done != 1 || mid != 0 {
        hprintln!("i2c-complete FAIL: done={} mid={}", done, mid);
        ok = false;
    } else {
        hprintln!("i2c-complete ok: Complete after N bytes, not before");
    }

    // 5) exclusive bus + phase gating (Kani p1/p4): a second START onto an in-flight
    //    transaction faults (BusBusy) without touching the bus, and `addr_ack` from a
    //    non-Addressing phase faults (WrongPhase). Both are mmio-free reject paths, so
    //    they return the fault sentinel deterministically. (`i2c_step` off-Active is
    //    NOT called here: the dissolved step polls SR1 unconditionally and would
    //    busy-wait rather than early-return the source's Err — a native/source
    //    divergence noted in RESULTS.md; step's phase gate is covered by the Kani p4
    //    source proof, not this native probe.)
    let busy = unsafe { i2c_start(base, s_active, 1, 0) };
    let ack_offphase = unsafe { i2c_addr_ack(s3) }; // s3 is Complete, not Addressing
    if busy != I2C_FAULT || ack_offphase != I2C_FAULT {
        hprintln!("i2c-fault FAIL: busy={:#x} ack_offphase={:#x}", busy, ack_offphase);
        ok = false;
    } else {
        hprintln!("i2c-fault ok: dup-START (busy bus) + addr_ack off-phase both fault");
    }

    // 6) STOP frees the bus back to Idle from any state (Kani p5), writing CR1.STOP.
    let idle = unsafe { i2c_stop(base, s_active) };
    let cr1_stop = unsafe { read_volatile(addr_of_mut!(REG[0])) };
    if idle != 0 || cr1_stop != CR1_STOP_LAST {
        hprintln!("i2c-stop FAIL: idle={:#x} CR1={:#x}", idle, cr1_stop);
        ok = false;
    } else {
        hprintln!("i2c-stop ok: bus back to Idle, CR1.STOP={:#x}", cr1_stop);
    }

    if ok {
        hprintln!("i2c-probe ALL OK");
        debug::exit(debug::EXIT_SUCCESS);
    } else {
        debug::exit(debug::EXIT_FAILURE);
    }
    loop {}
}
