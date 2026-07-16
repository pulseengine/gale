//! mpu_spike — 5-minute DECISION-POINT spike (pre-probe): does qemu's
//! lm3s6965evb (armv7m, cortex-m3) actually ENFORCE the v7-M MPU, or only
//! model the registers? Hand-programs (deliberately NOT via the verified
//! core — this is a platform capability check, not the oracle) three
//! regions: flash 256K, RAM-low 32K, RAM-stack 16K, leaving a 16K SRAM
//! hole at 0x2000_8000..0x2000_C000 that is physically backed but granted
//! to nobody. Enables the MPU with PRIVDEFENA=0 (deny-by-default), writes
//! inside a granted region (must succeed), then writes into the hole: if
//! qemu enforces, MemManage fires with MMFAR == the hole address and the
//! spike exits SUCCESS. If execution falls through the denied write, qemu
//! does not enforce -> exit FAILURE (and the probes must fall back to
//! Renode).
#![no_std]
#![no_main]

use core::ptr::{read_volatile, write_volatile};
use cortex_m_rt::{entry, exception};
use cortex_m_semihosting::{debug, hprintln};
use panic_halt as _;

const MPU_TYPE: *mut u32 = 0xE000_ED90 as *mut u32;
const MPU_CTRL: *mut u32 = 0xE000_ED94 as *mut u32;
const MPU_RNR: *mut u32 = 0xE000_ED98 as *mut u32;
const MPU_RBAR: *mut u32 = 0xE000_ED9C as *mut u32;
const MPU_RASR: *mut u32 = 0xE000_EDA0 as *mut u32;
const SHCSR: *mut u32 = 0xE000_ED24 as *mut u32;
const CFSR: *mut u32 = 0xE000_ED28 as *mut u32;
const MMFAR: *mut u32 = 0xE000_ED34 as *mut u32;

/// Physically-backed SRAM word no region grants (the 16K hole).
const DENIED_ADDR: u32 = 0x2000_8000;

static mut INSIDE_OK: bool = false;

#[allow(static_mut_refs)]
#[exception]
fn MemoryManagement() {
    let cfsr = unsafe { read_volatile(CFSR) };
    let mmfar = unsafe { read_volatile(MMFAR) };
    let inside = unsafe { INSIDE_OK };
    // Turn the MPU off before touching semihosting output paths.
    unsafe {
        write_volatile(MPU_CTRL, 0);
    }
    cortex_m::asm::dsb();
    cortex_m::asm::isb();
    if inside && mmfar == DENIED_ADDR && cfsr & 0x82 == 0x82 {
        // DACCVIOL + MMARVALID, faulting address is exactly the denied word.
        hprintln!(
            "mpu-spike OK: qemu ENFORCES v7-M MPU (inside-write ok; denied write -> MemManage, CFSR={:#010x} MMFAR={:#010x})",
            cfsr,
            mmfar
        );
        debug::exit(debug::EXIT_SUCCESS);
    } else {
        hprintln!(
            "mpu-spike FAIL: unexpected MemManage (inside_ok={} CFSR={:#010x} MMFAR={:#010x})",
            inside,
            cfsr,
            mmfar
        );
        debug::exit(debug::EXIT_FAILURE);
    }
    loop {}
}

#[exception]
unsafe fn HardFault(ef: &cortex_m_rt::ExceptionFrame) -> ! {
    unsafe {
        write_volatile(MPU_CTRL, 0);
    }
    cortex_m::asm::dsb();
    cortex_m::asm::isb();
    hprintln!(
        "mpu-spike FAIL: HardFault (MemManage escalated or unexpected) pc={:#010x} CFSR={:#010x} MMFAR={:#010x}",
        ef.pc(),
        unsafe { read_volatile(CFSR) },
        unsafe { read_volatile(MMFAR) }
    );
    debug::exit(debug::EXIT_FAILURE);
    loop {}
}

/// RASR: ENABLE | SIZE<<1 | AP<<24 (XN=0, TEX/C/B/S=0) — same encoding the
/// verified core emits.
fn rasr(size_log2: u32, ap: u32) -> u32 {
    1 | ((size_log2 - 1) << 1) | (ap << 24)
}

#[allow(static_mut_refs)]
#[entry]
fn main() -> ! {
    let dregion = (unsafe { read_volatile(MPU_TYPE) } >> 8) & 0xFF;
    if dregion != 8 {
        hprintln!("mpu-spike FAIL: MPU_TYPE.DREGION={} (want 8) -> no usable MPU on this qemu machine", dregion);
        debug::exit(debug::EXIT_FAILURE);
    }
    unsafe {
        // Enable the MemManage fault (else it escalates to HardFault).
        write_volatile(SHCSR, read_volatile(SHCSR) | (1 << 16));
        // MPU off while programming.
        write_volatile(MPU_CTRL, 0);
        cortex_m::asm::dsb();
        cortex_m::asm::isb();
        // r0: flash 0x0000_0000, 256K, RW (code + rodata + vectors).
        write_volatile(MPU_RNR, 0);
        write_volatile(MPU_RBAR, 0x0000_0000);
        write_volatile(MPU_RASR, rasr(18, 0b011));
        // r1: RAM low 0x2000_0000, 32K, RW (.data/.bss).
        write_volatile(MPU_RNR, 1);
        write_volatile(MPU_RBAR, 0x2000_0000);
        write_volatile(MPU_RASR, rasr(15, 0b011));
        // r2: RAM stack window 0x2000_C000, 16K, RW.
        write_volatile(MPU_RNR, 2);
        write_volatile(MPU_RBAR, 0x2000_C000);
        write_volatile(MPU_RASR, rasr(14, 0b011));
        // r3..r7 explicitly disabled.
        for r in 3..8 {
            write_volatile(MPU_RNR, r);
            write_volatile(MPU_RBAR, 0);
            write_volatile(MPU_RASR, 0);
        }
        // Enable: ENABLE only, PRIVDEFENA=0 -> deny-by-default.
        write_volatile(MPU_CTRL, 1);
        cortex_m::asm::dsb();
        cortex_m::asm::isb();

        // Direction 1: write INSIDE a granted region must succeed.
        INSIDE_OK = false;
        write_volatile(&mut INSIDE_OK as *mut bool, true);
        if !read_volatile(&INSIDE_OK as *const bool) {
            write_volatile(MPU_CTRL, 0);
            hprintln!("mpu-spike FAIL: inside-write did not land");
            debug::exit(debug::EXIT_FAILURE);
        }

        // Direction 2: write into the physically-backed but ungranted hole.
        // If qemu enforces, this never falls through.
        write_volatile(DENIED_ADDR as *mut u32, 0xDEAD_BEEF);
        // Fell through: no fault -> qemu does NOT enforce the MPU.
        write_volatile(MPU_CTRL, 0);
        cortex_m::asm::dsb();
        cortex_m::asm::isb();
    }
    hprintln!("mpu-spike FAIL: denied write FELL THROUGH -> qemu does NOT enforce the v7-M MPU on this machine");
    debug::exit(debug::EXIT_FAILURE);
    loop {}
}
