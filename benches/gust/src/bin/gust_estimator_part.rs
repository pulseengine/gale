//! gust_estimator_part — core-1 image of the v0.6.0 MULTI-CORE Renode
//! placement demo (renode-test/gust_switch_2core.repl + .robot): the
//! ESTIMATOR partition pinned to its own core, running concurrently with the
//! flight/mission/payload major frame on core 0 (gust_switch_renode.rs).
//! This is the REQ-OS-SWITCH-001 placement shape: multi-partition-per-core on
//! core 0, a dedicated estimator partition on core 1, each core's outer
//! switch/map independent.
//!
//! What this image does:
//!   1. builds ITS OWN region table exclusively through the VERIFIED builder
//!      (gale::mpu_switch::RegionTable::new + try_add_region) and programs
//!      the core-1 MPU exclusively through the VERIFIED switch_to_partition
//!      — then confirms the map is live by reading the REAL MPU register
//!      state back (RNR := 3, RBAR == this partition's scratch base;
//!      MPU_CTRL == enabled). Register STATE readback is spike-proven on
//!      this platform (src/bin/mpu_spike_renode.rs); ENFORCEMENT is not
//!      modelled by Renode's M3 — that evidence is qemu's (mpu_spike.rs,
//!      gust_switch_probe.rs).
//!   2. runs a periodic estimator computation (fixed-point first-order
//!      low-pass converging on a constant "measurement") and emits one
//!      liveness heartbeat line per period over core 1's OWN UART; the
//!      convergence is checked in-image (monotone error decrease + final
//!      error bound) so the printed heartbeats are backed by a computed
//!      result, not just a counter.
//!
//! Output: STM32 UART at 0x4001_3800 — core 1's own uart1 (per-core bus
//! registration in the .repl). Ends in a quiet WFI loop after the final
//! OK/FAIL line; no semihosting anywhere.
#![no_std]
#![no_main]

use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use cortex_m_rt::{entry, exception};
use gale::mpu_switch::{RegionTable, MPU_CTRL_ENABLE, REQUIRED_DREGION};
use panic_halt as _;

// ---------------------------------------------------------------------------
// ARMv7-M System Control Space (PPB).
// ---------------------------------------------------------------------------
const MPU_TYPE: *mut u32 = 0xE000_ED90 as *mut u32;
const MPU_CTRL: *mut u32 = 0xE000_ED94 as *mut u32;
const MPU_RNR: *mut u32 = 0xE000_ED98 as *mut u32;
const MPU_RBAR: *mut u32 = 0xE000_ED9C as *mut u32;
const MPU_RASR: *mut u32 = 0xE000_EDA0 as *mut u32;
const SHCSR: *mut u32 = 0xE000_ED24 as *mut u32;

const MPU_CTRL_ID: u32 = 0xFFFF_FFFF;

/// The estimator partition's id in ITS OWN core-local table.
const EST_PART: u32 = 0;

/// Core-1 map (same device class as core 0: 256K flash, 64K SRAM):
const FLASH_BASE: u32 = 0x0000_0000;
const FLASH_SIZE: u32 = 0x0004_0000; // 256K RO
const DATA_BASE: u32 = 0x2000_0000;
const DATA_SIZE: u32 = 0x0000_8000; // 32K RW
const STACK_BASE: u32 = 0x2000_C000;
const STACK_SIZE: u32 = 0x0000_4000; // 16K RW
/// Estimator scratch region (slot 3 — the RBAR-readback slot, mirroring the
/// core-0 demo's per-partition scratch shape).
const SCRATCH_BASE: u32 = 0x2000_8000;
const SCRATCH_SIZE: u32 = 0x800;
const SCRATCH_ADDR: u32 = 0x2000_8010;
/// Core 1's own UART window (reporting grant, slot 4).
const UART_BASE: u32 = 0x4001_3800;
const UART_WIN: u32 = 0x100;

/// Heartbeats the robot gate waits for.
const N_HEARTBEATS: u32 = 8;

// ---------------------------------------------------------------------------
// UART (STM32 F1 USART layout; Renode UART.STM32_UART — core 1's uart1).
// ---------------------------------------------------------------------------
const UART_SR: *mut u32 = 0x4001_3800 as *mut u32;
const UART_DR: *mut u32 = 0x4001_3804 as *mut u32;
const UART_BRR: *mut u32 = 0x4001_3808 as *mut u32;
const UART_CR1: *mut u32 = 0x4001_380C as *mut u32;

fn uart_init() {
    unsafe {
        write_volatile(UART_BRR, 0x45);
        write_volatile(UART_CR1, (1 << 13) | (1 << 3)); // UE | TE
    }
}

fn putb(b: u8) {
    unsafe {
        while read_volatile(UART_SR) & (1 << 7) == 0 {} // TXE
        write_volatile(UART_DR, b as u32);
    }
}

fn puts(s: &str) {
    for b in s.bytes() {
        putb(b);
    }
}

fn put_hex(v: u32) {
    puts("0x");
    let mut i = 0;
    while i < 8 {
        let nib = (v >> (28 - 4 * i)) & 0xF;
        putb(if nib < 10 { b'0' + nib as u8 } else { b'a' + (nib - 10) as u8 });
        i += 1;
    }
}

fn put_dec_small(v: u32) {
    if v >= 10 {
        putb(b'0' + (v / 10) as u8);
    }
    putb(b'0' + (v % 10) as u8);
}

fn putln(s: &str) {
    puts(s);
    putb(b'\n');
}

macro_rules! fail {
    ($msg:expr) => {{
        puts("gust-switch-2core core1 FAIL: ");
        putln($msg);
        loop {
            cortex_m::asm::wfi();
        }
    }};
}

// ---------------------------------------------------------------------------
// The `mpu_write` trusted-seam bridge (same contract as the core-0 image).
// ---------------------------------------------------------------------------
#[no_mangle]
pub extern "C" fn mpu_write(rnr: u32, rbar: u32, rasr: u32) {
    unsafe {
        if rnr == MPU_CTRL_ID {
            write_volatile(MPU_CTRL, rasr);
            cortex_m::asm::dsb();
            cortex_m::asm::isb();
        } else {
            write_volatile(MPU_RNR, rnr);
            write_volatile(MPU_RBAR, rbar);
            write_volatile(MPU_RASR, rasr);
        }
    }
}

// ---------------------------------------------------------------------------
// Faults: nothing may fault here — any exception is a demo failure.
// ---------------------------------------------------------------------------
#[exception]
fn MemoryManagement() {
    unsafe {
        write_volatile(MPU_CTRL, 0);
    }
    fail!("unexpected MemManage");
}

#[exception]
unsafe fn HardFault(_ef: &cortex_m_rt::ExceptionFrame) -> ! {
    unsafe {
        write_volatile(MPU_CTRL, 0);
    }
    fail!("HardFault");
}

static mut THE_TABLE: Option<RegionTable> = None;

fn grant(t: &mut RegionTable, base: u32, size: u32, writable: bool) {
    if !t.try_add_region(EST_PART, base, size, writable) {
        fail!("try_add_region rejected a grant");
    }
}

#[entry]
fn main() -> ! {
    uart_init();
    let dregion = (unsafe { read_volatile(MPU_TYPE) } >> 8) & 0xFF;
    if dregion != REQUIRED_DREGION {
        fail!("MPU_TYPE.DREGION != 8");
    }
    unsafe {
        write_volatile(SHCSR, read_volatile(SHCSR) | (1 << 16));
    }
    putln("gust-switch-2core core1 estimator begin dregion 8");

    // ---- The estimator partition's OWN map, verified builder only ---------
    let mut t = RegionTable::new();
    grant(&mut t, FLASH_BASE, FLASH_SIZE, false); // slot 0
    grant(&mut t, DATA_BASE, DATA_SIZE, true); // slot 1
    grant(&mut t, STACK_BASE, STACK_SIZE, true); // slot 2
    grant(&mut t, SCRATCH_BASE, SCRATCH_SIZE, true); // slot 3
    grant(&mut t, UART_BASE, UART_WIN, true); // slot 4
    if !t.covers_addr(EST_PART, SCRATCH_ADDR) {
        fail!("covers_addr: own scratch not covered");
    }
    unsafe {
        *addr_of_mut!(THE_TABLE) = Some(t);
    }

    // ---- Program the core-1 MPU exclusively through the VERIFIED path -----
    unsafe {
        match (*addr_of!(THE_TABLE)).as_ref() {
            Some(t) => t.switch_to_partition(EST_PART),
            None => fail!("table not built"),
        }
    }
    // Map-live confirmation from REAL register state (spike-proven readback):
    let (ctrl, rbar) = unsafe {
        write_volatile(MPU_RNR, 3);
        (read_volatile(MPU_CTRL), read_volatile(MPU_RBAR) & !0x1F)
    };
    if ctrl != MPU_CTRL_ENABLE {
        fail!("MPU_CTRL not enabled after verified switch");
    }
    if rbar != SCRATCH_BASE {
        fail!("RBAR readback != estimator scratch base");
    }
    // Silence the unused-RASR-const lint path: read RASR once too (state).
    let _ = unsafe { read_volatile(MPU_RASR) };
    puts("core1 estimator map-live rbar ");
    put_hex(rbar);
    putln(" mpu-ctrl-enabled");

    // ---- Periodic estimator computation + heartbeat ------------------------
    // Fixed-point (Q8) first-order low-pass est += (meas - est) >> 3,
    // converging on a constant measurement; per period the error must
    // strictly decrease (until saturation) and finally sit within a bound.
    let meas: i32 = 1000 << 8;
    let mut est: i32 = 0;
    let mut prev_err: i32 = i32::MAX;
    let mut hb: u32 = 1;
    while hb <= N_HEARTBEATS {
        // One "period" of estimator work: 32 filter steps + scratch traffic.
        let mut step = 0;
        while step < 32 {
            est += (meas - est) >> 3;
            step += 1;
        }
        unsafe {
            write_volatile(SCRATCH_ADDR as *mut u32, est as u32);
            if read_volatile(SCRATCH_ADDR as *const u32) != est as u32 {
                fail!("scratch write did not land");
            }
        }
        let err = if meas - est < 0 { est - meas } else { meas - est };
        if err > prev_err {
            fail!("estimator diverged (error increased)");
        }
        prev_err = err;
        puts("core1 estimator-hb ");
        put_dec_small(hb);
        puts(" est ");
        put_hex(est as u32);
        putb(b'\n');
        hb += 1;
    }
    // After 8 * 32 steps of est += err/8 the filter must have converged.
    if prev_err > 1 << 8 {
        fail!("estimator did not converge");
    }
    putln(
        "gust-switch-2core core1 OK: estimator partition on its own core, map programmed via verified switch_to_partition (RBAR readback), 8 heartbeats, converged",
    );
    loop {
        cortex_m::asm::wfi();
    }
}
