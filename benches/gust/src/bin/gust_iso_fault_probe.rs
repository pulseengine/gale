//! gust-iso-fault-probe — the v0.5.0 I-ISO fault-injection oracle: the verified
//! MPU region-programming core (gale src/mpu_switch.rs, Verus 1098/0 + Kani 4/4)
//! demonstrated as a REAL hardware isolation boundary on qemu's v7-M MPU
//! (lm3s6965evb, cortex-m3 — enforcement pre-verified by src/bin/mpu_spike.rs).
//!
//! The MPU is programmed EXCLUSIVELY through the verified path:
//! `RegionTable::switch_to_partition(0)` → `program_partition` (P1–P4 proven)
//! → `apply_program` → the `mpu_write` trusted seam, which THIS probe
//! implements faithfully per its documented platform contract:
//!
//!   * contract item 1 — DSB+ISB after every MPU_CTRL write (both the leading
//!     disable and the trailing enable), so the proven P4 ordering actually
//!     reaches the hardware;
//!   * contract item 2 — init-time `MPU_TYPE.DREGION == REQUIRED_DREGION (8)`
//!     check that REFUSES to start (semihosting FAIL) on any other part, the
//!     stale-slots-above-8 hazard the module header names.
//!
//! Toy partition 0 (deny-by-default — MPU_CTRL_ENABLE has PRIVDEFENA clear,
//! so even privileged code gets NOTHING the table does not grant):
//!
//!   region 0: flash 0x0000_0000 256K RO  (code + rodata + vectors)
//!   region 1: SRAM  0x2000_0000 32K  RW  (.data/.bss)
//!   region 2: SRAM  0x2000_C000 16K  RW  (stack window; MSP init 0x2001_0000)
//!   regions 3–7: emitted DISABLED by the verified core (P2 deny-by-default)
//!
//! leaving [0x2000_8000, 0x2000_C000) physically-backed SRAM granted to NOBODY.
//!
//! The oracle asserts BOTH directions:
//!   1. a write INSIDE region 1 lands (read-back verified);
//!   2. a write to DENIED 0x2000_8000 MemManage-FAULTS, with the recorded
//!      MMFAR == the denied address and CFSR showing DACCVIOL+MMARVALID.
//!
//! Fault recovery: a naked MemoryManagement handler records MMFAR/CFSR and
//! returns via a MODIFIED STACKED PC to a declared resume continuation (the
//! "recover" arm of the handler contract); any fault with no resume armed is
//! reported and FAILed. Exit codes: semihosting EXIT_SUCCESS / EXIT_FAILURE.
#![no_std]
#![no_main]

use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use core::sync::atomic::{compiler_fence, Ordering};
use cortex_m_rt::{entry, exception};
use cortex_m_semihosting::{debug, hprintln};
use gale::mpu_switch::{RegionTable, MPU_CTRL_ENABLE, REQUIRED_DREGION};
use panic_halt as _;

// ---------------------------------------------------------------------------
// ARMv7-M System Control Space (PPB — always accessible, never MPU-checked).
// ---------------------------------------------------------------------------
const MPU_TYPE: *mut u32 = 0xE000_ED90 as *mut u32;
const MPU_CTRL: *mut u32 = 0xE000_ED94 as *mut u32;
const MPU_RNR: *mut u32 = 0xE000_ED98 as *mut u32;
const MPU_RBAR: *mut u32 = 0xE000_ED9C as *mut u32;
const MPU_RASR: *mut u32 = 0xE000_EDA0 as *mut u32;
const SHCSR: *mut u32 = 0xE000_ED24 as *mut u32;
const CFSR: *mut u32 = 0xE000_ED28 as *mut u32;
const MMFAR: *mut u32 = 0xE000_ED34 as *mut u32;

/// Sentinel the verified core uses for MPU_CTRL writes (mpu_switch::MPU_CTRL_ID).
const MPU_CTRL_ID: u32 = 0xFFFF_FFFF;

/// Physically-backed SRAM word partition 0's table grants to nobody.
const DENIED_ADDR: u32 = 0x2000_8000;

// ---------------------------------------------------------------------------
// The `mpu_write` trusted-seam bridge — the ONLY register-store path, exactly
// per the seam's platform contract in src/mpu_switch.rs.
// ---------------------------------------------------------------------------

/// Platform implementation of the verified core's trusted extern seam.
/// `rnr == MPU_CTRL_ID` → MPU_CTRL := rasr, then DSB+ISB (contract item 1);
/// otherwise RNR := rnr, RBAR := rbar, RASR := rasr.
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

/// Init-time platform check (contract item 2): refuse to start unless
/// `MPU_TYPE.DREGION == REQUIRED_DREGION` — on a 16-region part the verified
/// sequence would leave slots 8..=15 STALE, silently defeating P2.
fn mpu_bridge_init_check() {
    let dregion = (unsafe { read_volatile(MPU_TYPE) } >> 8) & 0xFF;
    if dregion != REQUIRED_DREGION {
        hprintln!(
            "gust-iso-fault-probe FAIL: platform contract violated — MPU_TYPE.DREGION={} (require {}); refusing to start",
            dregion,
            REQUIRED_DREGION
        );
        debug::exit(debug::EXIT_FAILURE);
    }
}

// ---------------------------------------------------------------------------
// MemManage fault recording + resume-continuation recovery.
// ---------------------------------------------------------------------------

static mut FAULT_COUNT: u32 = 0;
static mut FAULT_MMFAR: u32 = 0;
static mut FAULT_CFSR: u32 = 0;
/// Thumb address of the armed resume continuation (0 = none armed: any
/// MemManage is unexpected and FAILs the probe from the handler).
static mut RESUME_PC: u32 = 0;
static mut INSIDE_OK: u32 = 0;

// Naked MemoryManagement handler: on exception entry MSP points at the
// hardware-stacked frame {r0,r1,r2,r3,r12,lr,pc,xpsr}; hand that pointer to
// the Rust recorder, which returns through the untouched EXC_RETURN in lr.
core::arch::global_asm!(
    ".section .text.MemoryManagement",
    ".global MemoryManagement",
    ".thumb_func",
    "MemoryManagement:",
    "    mrs r0, msp",
    "    b   iso_memmanage",
);

/// Record the fault, clear the sticky MemManage status bits, and REDIRECT the
/// stacked PC to the armed resume continuation (clearing the stacked ICI/IT
/// bits and forcing Thumb state, so the exception return is architecturally
/// clean). No resume armed → report + FAIL from the handler.
#[no_mangle]
extern "C" fn iso_memmanage(frame: *mut u32) {
    unsafe {
        let cfsr = read_volatile(CFSR);
        let mmfar = read_volatile(MMFAR);
        write_volatile(addr_of_mut!(FAULT_CFSR), cfsr);
        write_volatile(addr_of_mut!(FAULT_MMFAR), mmfar);
        let n = read_volatile(addr_of!(FAULT_COUNT));
        write_volatile(addr_of_mut!(FAULT_COUNT), n + 1);
        // Clear the sticky MemManage status byte (write-1-to-clear).
        write_volatile(CFSR, 0xFF);
        let resume = read_volatile(addr_of!(RESUME_PC));
        if resume == 0 {
            // Unexpected fault: not part of the oracle. Post-oracle cleanup:
            // MPU off (direct write — the oracle already failed), report, exit.
            write_volatile(MPU_CTRL, 0);
            cortex_m::asm::dsb();
            cortex_m::asm::isb();
            hprintln!(
                "gust-iso-fault-probe FAIL: unexpected MemManage (no resume armed) pc={:#010x} CFSR={:#010x} MMFAR={:#010x}",
                read_volatile(frame.add(6)),
                cfsr,
                mmfar
            );
            debug::exit(debug::EXIT_FAILURE);
            loop {}
        }
        write_volatile(addr_of_mut!(RESUME_PC), 0);
        // Stacked PC := continuation (bit 0 clear); stacked xPSR: clear
        // ICI/IT (bits 26:25, 15:10), force T (bit 24).
        write_volatile(frame.add(6), resume & !1);
        let xpsr = read_volatile(frame.add(7));
        write_volatile(frame.add(7), (xpsr & !0x0600_FC00) | (1 << 24));
    }
}

#[exception]
unsafe fn HardFault(ef: &cortex_m_rt::ExceptionFrame) -> ! {
    unsafe {
        write_volatile(MPU_CTRL, 0);
    }
    cortex_m::asm::dsb();
    cortex_m::asm::isb();
    hprintln!(
        "gust-iso-fault-probe FAIL: HardFault (MemManage escalated?) pc={:#010x} CFSR={:#010x} MMFAR={:#010x}",
        ef.pc(),
        unsafe { read_volatile(CFSR) },
        unsafe { read_volatile(MMFAR) }
    );
    debug::exit(debug::EXIT_FAILURE);
    loop {}
}

/// Resume continuation for the denied write: entered ONLY via the handler's
/// stacked-PC redirect. Judges the oracle and exits. Never returns (the
/// interrupted instruction is abandoned, not retried).
#[no_mangle]
extern "C" fn denied_write_faulted() -> ! {
    let (n, mmfar, cfsr, inside) = unsafe {
        (
            read_volatile(addr_of!(FAULT_COUNT)),
            read_volatile(addr_of!(FAULT_MMFAR)),
            read_volatile(addr_of!(FAULT_CFSR)),
            read_volatile(addr_of!(INSIDE_OK)),
        )
    };
    // DACCVIOL (bit 1) + MMARVALID (bit 7): a data-access violation with a
    // valid faulting-address register.
    let ok = inside == 1 && n == 1 && mmfar == DENIED_ADDR && cfsr & 0x82 == 0x82;
    if ok {
        hprintln!(
            "gust-iso-fault-probe OK: inside-write ok, outside-write denied @{:#010x} (CFSR={:#010x} DACCVIOL+MMARVALID; MPU programmed via verified switch_to_partition)",
            mmfar,
            cfsr
        );
        debug::exit(debug::EXIT_SUCCESS);
    } else {
        hprintln!(
            "gust-iso-fault-probe FAIL: fault recorded but wrong shape (inside_ok={} faults={} MMFAR={:#010x} want {:#010x} CFSR={:#010x})",
            inside,
            n,
            mmfar,
            DENIED_ADDR,
            cfsr
        );
        debug::exit(debug::EXIT_FAILURE);
    }
    loop {}
}

#[entry]
fn main() -> ! {
    // Platform contract, item 2 — refuse to start on a non-8-region part.
    mpu_bridge_init_check();
    unsafe {
        // Enable the MemManage fault so a violation does not escalate.
        write_volatile(SHCSR, read_volatile(SHCSR) | (1 << 16));
    }

    // Toy partition 0 — built as data, programmed via the VERIFIED path.
    // (RegionTable::new() is the all-disabled deny-everything baseline; the
    // three grants satisfy table_inv: power-of-2 sizes >= 32, size-aligned
    // bases, pairwise-disjoint ranges.)
    let mut t = RegionTable::new();
    // region 0: flash 256K, read-only (writable=false → AP=0b110).
    t.base[0] = 0x0000_0000;
    t.size[0] = 0x0004_0000;
    t.enabled[0] = true;
    t.writable[0] = false;
    // region 1: SRAM low 32K, RW (.data/.bss).
    t.base[1] = 0x2000_0000;
    t.size[1] = 0x0000_8000;
    t.enabled[1] = true;
    t.writable[1] = true;
    // region 2: SRAM stack window 16K at 0x2000_C000, RW.
    t.base[2] = 0x2000_C000;
    t.size[2] = 0x0000_4000;
    t.enabled[2] = true;
    t.writable[2] = true;

    // THE verified path: program_partition (P1–P4 proven) → apply_program →
    // mpu_write seam. No hand-programming anywhere in this probe.
    t.switch_to_partition(0);

    // Sanity: the trailing enable write reached MPU_CTRL (ENABLE set,
    // PRIVDEFENA clear — deny-by-default is live).
    let ctrl = unsafe { read_volatile(MPU_CTRL) };
    if ctrl != MPU_CTRL_ENABLE {
        hprintln!(
            "gust-iso-fault-probe FAIL: MPU_CTRL={:#x} after verified switch (want {:#x})",
            ctrl,
            MPU_CTRL_ENABLE
        );
        debug::exit(debug::EXIT_FAILURE);
    }

    unsafe {
        // Direction 1: a write INSIDE granted region 1 must land.
        write_volatile(addr_of_mut!(INSIDE_OK), 1);
        if read_volatile(addr_of!(INSIDE_OK)) != 1 {
            hprintln!("gust-iso-fault-probe FAIL: inside-write did not land");
            debug::exit(debug::EXIT_FAILURE);
        }

        // Direction 2: arm the resume continuation, then write the denied
        // word. If the MPU enforces, execution continues at
        // denied_write_faulted and the lines below are never reached.
        write_volatile(
            addr_of_mut!(RESUME_PC),
            denied_write_faulted as *const () as u32,
        );
        compiler_fence(Ordering::SeqCst);
        write_volatile(DENIED_ADDR as *mut u32, 0xDEAD_BEEF);
        compiler_fence(Ordering::SeqCst);

        // Fell through: the denied write was NOT trapped.
        write_volatile(MPU_CTRL, 0);
        cortex_m::asm::dsb();
        cortex_m::asm::isb();
    }
    hprintln!(
        "gust-iso-fault-probe FAIL: write to denied {:#010x} fell through — MPU not enforcing",
        DENIED_ADDR
    );
    debug::exit(debug::EXIT_FAILURE);
    loop {}
}
