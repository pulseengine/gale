//! gust-wdg-silicon — REAL-HARDWARE anchor for the dissolved wdg-thin IWDG driver.
//!
//! Unlike gust_wdg_probe (qemu, RAM-window fake registers) and gust_wdg (Renode
//! content-gate), this firmware points the DISSOLVED wdg-thin-cm3.o at the REAL
//! IWDG peripheral (0x4000_3000) and proves the watchdog actually fires on silicon.
//!
//! Target: NUCLEO-G474RE (Cortex-M4, thumbv7em; the cortex-m3 .o runs unmodified,
//! thumbv7m ⊂ thumbv7em). The IWDG is register-identical across the STM32 line
//! (same base, same KR/PR/RLR key sequence), so the F1-authored driver programs
//! the G4 watchdog verbatim — the one peripheral that is faithfully testable on
//! whichever board is at hand. Flash + capture:  benches/gust/silicon/run-wdg.sh
//!
//! Two-boot proof (self-checking, no scope/LED needed):
//!   boot 1 — RCC_CSR.IWDGRSTF == 0: arm the IWDG via the driver, then STOP kicking
//!            it. With no refresh the hardware watchdog times out (~1.2s) and resets
//!            the whole chip.
//!   boot 2 — after the reset, RCC_CSR.IWDGRSTF == 1: the watchdog demonstrably
//!            fired on real silicon. Report OK, clear the flag, halt.
//! A driver that silently no-op'd the start (KR=0xCCCC) would never reset → boot 2
//! never happens → the test cannot false-pass.
#![no_std]
#![no_main]

use core::ptr::{read_volatile, write_volatile};
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use panic_halt as _;

// The mmio seam the dissolved driver imports — here it drives the REAL peripheral.
#[no_mangle]
pub extern "C" fn mmio_read32(addr: u32) -> u32 {
    unsafe { read_volatile(addr as *const u32) }
}
#[no_mangle]
pub extern "C" fn mmio_write32(addr: u32, val: u32) {
    unsafe { write_volatile(addr as *mut u32, val) }
}

// The dissolved wdg-thin driver (state-threaded FSM; writes to `base`).
extern "C" {
    fn wdg_unlock(base: u32, state: u32) -> u32;
    fn wdg_configure(base: u32, state: u32, prescaler: u32, reload: u32) -> u32;
    fn wdg_lock(state: u32) -> u32;
    fn wdg_start(base: u32, state: u32) -> u32;
    fn wdg_is_running(state: u32) -> u32;
}

const IWDG_BASE: u32 = 0x4000_3000; // universal STM32 IWDG base (F1 == G4)
const KEY_START: u32 = 0xCCCC;
const WDG_FAULT: u32 = 0xFFFF_FFFF;
const PRESCALER: u32 = 5; // ÷128 of the ~32 kHz LSI  → 250 Hz tick
const RELOAD: u32 = 0x123; // 291 ticks ≈ 1.16 s timeout (same values gust_wdg_probe proves)

// STM32G4 RCC control/status register (differs from F1's 0x4002_1024 offset).
const RCC_CSR: u32 = 0x4002_1094;
const IWDGRSTF: u32 = 1 << 29; // independent-watchdog reset flag
const RMVF: u32 = 1 << 23; // remove/clear reset flags

#[entry]
fn main() -> ! {
    let csr = unsafe { read_volatile(RCC_CSR as *const u32) };

    if csr & IWDGRSTF != 0 {
        // boot 2: the watchdog fired on real silicon.
        hprintln!(
            "gust-wdg-silicon OK: IWDG watchdog reset CONFIRMED on real G474RE silicon \
             (RCC_CSR=0x{:08x}, IWDGRSTF=1) — the dissolved wdg-thin driver armed the \
             hardware watchdog and it fired the reset.",
            csr
        );
        // clear the reset flags so a re-run starts clean.
        unsafe { write_volatile(RCC_CSR as *mut u32, csr | RMVF) };
        debug::exit(debug::EXIT_SUCCESS);
        loop {}
    }

    // boot 1: arm the real IWDG through the dissolved driver, then stop kicking it.
    hprintln!(
        "gust-wdg-silicon: boot 1 (RCC_CSR=0x{:08x}, no prior WDG reset). Arming the REAL \
         IWDG @0x{:08x} via the dissolved wdg-thin driver (PR={}, RLR=0x{:x} ≈ 1.2 s)...",
        csr, IWDG_BASE, PRESCALER, RELOAD
    );

    let s1 = unsafe { wdg_unlock(IWDG_BASE, 0) };
    let s2 = unsafe { wdg_configure(IWDG_BASE, s1, PRESCALER, RELOAD) };
    let s3 = unsafe { wdg_lock(s2) };
    let s4 = unsafe { wdg_start(IWDG_BASE, s3) };

    let kr = unsafe { read_volatile(IWDG_BASE as *const u32) }; // KR is write-only; readback is the driver's last write on this bus model
    let running = unsafe { wdg_is_running(s4) };
    if s4 == WDG_FAULT || running != 1 {
        hprintln!(
            "gust-wdg-silicon FAIL: driver did not reach Running (s4=0x{:08x}, is_running={}) \
             — the dissolved start (KR=0x{:04x}) did not take.",
            s4, running, KEY_START
        );
        debug::exit(debug::EXIT_FAILURE);
        loop {}
    }

    hprintln!(
        "gust-wdg-silicon: armed (last KR write=0x{:04x}, is_running=1). NOT refreshing — \
         expect a HARDWARE reset in ~1.2 s, after which boot 2 reads IWDGRSTF=1.",
        kr & 0xFFFF
    );

    // Deliberately never refresh: the hardware IWDG times out and resets the chip.
    loop {
        cortex_m::asm::nop();
    }
}
