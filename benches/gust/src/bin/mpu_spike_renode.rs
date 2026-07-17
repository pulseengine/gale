//! mpu_spike_renode — DECISION-POINT spike for the Renode multi-core switch
//! demo (the Renode counterpart of mpu_spike.rs): does Renode's Cortex-M3
//! model actually ENFORCE the v7-M PMSA MPU, or only hold the registers as
//! state, or not model them at all? Hand-programs (deliberately NOT via the
//! verified core — this is a platform capability check, not the oracle) the
//! same three regions as mpu_spike.rs, leaving the 16K SRAM hole at
//! 0x2000_8000..0x2000_C000 granted to nobody, then writes into the hole.
//!
//! Renode-specific reporting: NO semihosting anywhere (the macOS Renode
//! portable captures no SemihostingUart output headless) — every step and the
//! final verdict are recorded as MAGIC WORDS at fixed SRAM addresses inside
//! the always-granted low-RAM region, read back from the Renode monitor with
//! `sysbus ReadDoubleWord` after `RunFor`. The three possible verdicts:
//!
//!   RESULT = 0x600D0082  ENFORCED   — denied write raised MemManage with
//!                                     CFSR DACCVIOL+MMARVALID and MMFAR ==
//!                                     the hole address (qemu-equivalent);
//!   RESULT = 0xBADF0011  FELL-THROUGH — the denied store landed: the MPU is
//!                                     (at most) register state, no
//!                                     enforcement;
//!   RESULT = 0xBAD0D4E6  NO-MPU     — MPU_TYPE.DREGION != 8: no usable v7-M
//!                                     MPU on this model at all.
//!
//! Register-STATE is probed separately (works even without enforcement):
//! RBAR/RASR are written and read back before the enable; REG_RBAR/REG_RASR
//! hold the readback so the fallback demo ("swap observed via readback") can
//! be justified — or ruled out — from the same spike.
#![no_std]
#![no_main]

use core::ptr::{read_volatile, write_volatile};
use cortex_m_rt::{entry, exception};
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

/// Result mailbox (inside the always-granted 32K low-RAM region, well above
/// this spike's tiny .data/.bss and below the stack window).
const OUT_RESULT: *mut u32 = 0x2000_7E00 as *mut u32; // final verdict
const OUT_STEP: *mut u32 = 0x2000_7E04 as *mut u32; // progress marker
const OUT_DREGION: *mut u32 = 0x2000_7E08 as *mut u32; // MPU_TYPE.DREGION
const OUT_REG_RBAR: *mut u32 = 0x2000_7E0C as *mut u32; // RBAR readback (state probe)
const OUT_REG_RASR: *mut u32 = 0x2000_7E10 as *mut u32; // RASR readback (state probe)
const OUT_CFSR: *mut u32 = 0x2000_7E14 as *mut u32; // fault CFSR
const OUT_MMFAR: *mut u32 = 0x2000_7E18 as *mut u32; // fault MMFAR

const STEP_STARTED: u32 = 0x51AE_0001;
const STEP_INSIDE_OK: u32 = 0x51AE_0002;
const STEP_DENIED_ISSUED: u32 = 0x51AE_0003;

const RESULT_ENFORCED: u32 = 0x600D_0082;
const RESULT_FELL_THROUGH: u32 = 0xBADF_0011;
const RESULT_NO_MPU: u32 = 0xBAD0_D4E6;
const RESULT_WRONG_FAULT: u32 = 0xBAD0_0F17;
const RESULT_HARDFAULT: u32 = 0xBAD0_44F7;
const RESULT_INSIDE_LOST: u32 = 0xBAD0_1057;

fn out(slot: *mut u32, val: u32) {
    unsafe { write_volatile(slot, val) };
}

#[exception]
fn MemoryManagement() {
    let cfsr = unsafe { read_volatile(CFSR) };
    let mmfar = unsafe { read_volatile(MMFAR) };
    unsafe {
        write_volatile(MPU_CTRL, 0);
    }
    cortex_m::asm::dsb();
    cortex_m::asm::isb();
    out(OUT_CFSR, cfsr);
    out(OUT_MMFAR, mmfar);
    let armed = unsafe { read_volatile(OUT_STEP) } == STEP_DENIED_ISSUED;
    if armed && mmfar == DENIED_ADDR && cfsr & 0x82 == 0x82 {
        // DACCVIOL + MMARVALID, faulting address is exactly the denied word:
        // Renode ENFORCES the v7-M MPU, qemu-equivalent shape.
        out(OUT_RESULT, RESULT_ENFORCED);
    } else {
        out(OUT_RESULT, RESULT_WRONG_FAULT);
    }
    // Never return (a MemManage return would re-execute the store with the
    // MPU now off and overwrite the verdict via the fell-through path).
    loop {
        cortex_m::asm::wfi();
    }
}

#[exception]
unsafe fn HardFault(_ef: &cortex_m_rt::ExceptionFrame) -> ! {
    unsafe {
        write_volatile(MPU_CTRL, 0);
    }
    cortex_m::asm::dsb();
    cortex_m::asm::isb();
    out(OUT_CFSR, unsafe { read_volatile(CFSR) });
    out(OUT_MMFAR, unsafe { read_volatile(MMFAR) });
    out(OUT_RESULT, RESULT_HARDFAULT);
    loop {
        cortex_m::asm::wfi();
    }
}

/// RASR: ENABLE | SIZE<<1 | AP<<24 (XN=0, TEX/C/B/S=0) — same encoding the
/// verified core emits.
fn rasr(size_log2: u32, ap: u32) -> u32 {
    1 | ((size_log2 - 1) << 1) | (ap << 24)
}

#[entry]
fn main() -> ! {
    out(OUT_RESULT, 0);
    out(OUT_STEP, STEP_STARTED);
    let dregion = (unsafe { read_volatile(MPU_TYPE) } >> 8) & 0xFF;
    out(OUT_DREGION, dregion);
    if dregion != 8 {
        out(OUT_RESULT, RESULT_NO_MPU);
        loop {
            cortex_m::asm::wfi();
        }
    }
    unsafe {
        // Enable the MemManage fault (else it escalates to HardFault).
        write_volatile(SHCSR, read_volatile(SHCSR) | (1 << 16));
        // MPU off while programming.
        write_volatile(MPU_CTRL, 0);
        cortex_m::asm::dsb();
        cortex_m::asm::isb();
        // Register-STATE probe (independent of enforcement): program slot 3,
        // read RBAR/RASR back, record. The fallback demo's "swap observed via
        // readback" is only claimable if this readback holds the written value.
        write_volatile(MPU_RNR, 3);
        write_volatile(MPU_RBAR, DENIED_ADDR);
        write_volatile(MPU_RASR, rasr(11, 0b011)); // 2 KiB RW (probe value only)
        out(OUT_REG_RBAR, read_volatile(MPU_RBAR));
        out(OUT_REG_RASR, read_volatile(MPU_RASR));
        // Clear the probe slot again, then the real spike map.
        write_volatile(MPU_RNR, 3);
        write_volatile(MPU_RBAR, 0);
        write_volatile(MPU_RASR, 0);
        // r0: flash 0x0000_0000, 256K, RW (code + rodata + vectors).
        write_volatile(MPU_RNR, 0);
        write_volatile(MPU_RBAR, 0x0000_0000);
        write_volatile(MPU_RASR, rasr(18, 0b011));
        // r1: RAM low 0x2000_0000, 32K, RW (.data/.bss + the result mailbox).
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
        const INSIDE_MAGIC: u32 = 0x1751_DE0C;
        let canary: *mut u32 = 0x2000_7E80 as *mut u32;
        write_volatile(canary, INSIDE_MAGIC);
        if read_volatile(canary) != INSIDE_MAGIC {
            write_volatile(MPU_CTRL, 0);
            out(OUT_RESULT, RESULT_INSIDE_LOST);
            loop {
                cortex_m::asm::wfi();
            }
        }
        out(OUT_STEP, STEP_INSIDE_OK);

        // Direction 2: write into the physically-backed but ungranted hole.
        // If Renode enforces, this never falls through.
        out(OUT_STEP, STEP_DENIED_ISSUED);
        write_volatile(DENIED_ADDR as *mut u32, 0xDEAD_BEEF);
        // Fell through: no fault -> Renode does NOT enforce the MPU.
        write_volatile(MPU_CTRL, 0);
        cortex_m::asm::dsb();
        cortex_m::asm::isb();
    }
    out(OUT_RESULT, RESULT_FELL_THROUGH);
    loop {
        cortex_m::asm::wfi();
    }
}
