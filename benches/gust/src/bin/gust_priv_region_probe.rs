//! gust-priv-region-probe — gale#410's privilege axis, EXECUTED on hardware.
//!
//! The model can now express "supervisor-private": a region added with
//! `UNPRIV_NONE` is emitted with an AP encoding that grants unprivileged code
//! no access (Verus proves the emission, Kani cross-checks it, and its own
//! negative control `iso_unmarked_slots_stay_unprivileged_accessible` proves
//! that check is not satisfiable by an encoder that denies everything).
//!
//! **None of that says the hardware honours the encoding.** That is an ARMv7-M
//! architectural fact, assumed exactly as the `mpu_write` seam contract is.
//! This probe discharges it the only way it can be discharged — by running it.
//!
//! MATCHED PAIR, and the pair is the point. Exactly one arm is built:
//!
//!   priv-private : the target word's region is added with `UNPRIV_NONE`.
//!                  The unprivileged read MUST fault, MMFAR MUST name the
//!                  target address, and the fault MUST be a MemManage
//!                  (not an escalated HardFault).
//!   priv-shared  : the SAME image, the SAME regions, the SAME instruction —
//!                  only the target region's `unpriv` differs (`UNPRIV_SAME`).
//!                  The read MUST succeed and return the magic.
//!
//! A single green on `priv-private` would prove the platform, not the marking:
//! a read can fault because the region was never mapped, because the base was
//! wrong, or because the MPU denied it for any other reason. `priv-shared` is
//! what makes the fault attributable to the one field that changed. This is the
//! same shape the two-tenant criterion uses (gale#399), for the same reason.
//!
//! NOT claimed: that gust tenants run unprivileged today (REQ-OS-UNPRIV-001 is
//! open — see `gust_iso_unpriv_probe`, whose polarity records that gap). This
//! probe drops to `CONTROL.nPRIV=1` itself, for one read, to establish that the
//! encoding the verified model now emits does what the model says it does.
#![no_std]
#![no_main]

use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use cortex_m_semihosting::{debug, hprintln};
use cortex_m_rt::entry;
use gale::mpu_switch::{RegionTable, MPU_CTRL_ENABLE, REQUIRED_DREGION};
#[cfg(feature = "priv-private")]
use gale::mpu_switch::UNPRIV_NONE;
#[cfg(feature = "priv-shared")]
use gale::mpu_switch::UNPRIV_SAME;
use panic_halt as _;

#[cfg(all(feature = "priv-private", feature = "priv-shared"))]
compile_error!("gust_priv_region_probe: build exactly one arm — priv-private XOR priv-shared");
#[cfg(not(any(feature = "priv-private", feature = "priv-shared")))]
compile_error!("gust_priv_region_probe: pick an arm: --features priv-private | priv-shared");

const MPU_TYPE: *mut u32 = 0xE000_ED90 as *mut u32;
const MPU_CTRL: *mut u32 = 0xE000_ED94 as *mut u32;
const SHCSR: *mut u32 = 0xE000_ED24 as *mut u32;
const CFSR: *mut u32 = 0xE000_ED28 as *mut u32;
const MMFAR: *mut u32 = 0xE000_ED34 as *mut u32;

/// Code. Both bench boards (NUCLEO-G474RE, NUCLEO-WB55RG) map flash at
/// 0x0800_0000; 1 MiB covers the largest of them and is power-of-two aligned,
/// which `region_wf` requires.
const CODE_BASE: u32 = 0x0800_0000;
const CODE_SIZE: u32 = 1024 * 1024;

/// Stack and statics: the first 32 KiB of SRAM. Deliberately does NOT reach the
/// target word — an overlapping grant would be rejected by `try_add_region`,
/// and a region the tenant can already reach would make the experiment vacuous.
const RAM_BASE: u32 = 0x2000_0000;
const RAM_SIZE: u32 = 32 * 1024;

/// The word under test: its own 32-byte region (`MIN_REGION_SIZE`, 32-aligned),
/// above the stack region and inside SRAM1 on both boards.
const TARGET_BASE: u32 = 0x2000_8000;
const TARGET_SIZE: u32 = 32;
const MAGIC: u32 = 0x5157_2A10;

static mut FAULT_COUNT: u32 = 0;
static mut FAULT_MMFAR: u32 = 0;
static mut FAULT_CFSR: u32 = 0;
static mut READ_BACK: u32 = 0;
/// Thumb address of the armed resume continuation. 0 = none armed: any
/// MemManage is unexpected and fails the probe from inside the handler.
static mut RESUME_PC: u32 = 0;

/// The verified core's trusted `mpu_write` seam. Contract item 1: DSB+ISB after
/// every MPU_CTRL write, so the proven ordering (P4a/P4b) reaches hardware.
#[no_mangle]
pub extern "C" fn mpu_write(rnr: u32, rbar: u32, rasr: u32) {
    const MPU_RNR: *mut u32 = 0xE000_ED98 as *mut u32;
    const MPU_RBAR: *mut u32 = 0xE000_ED9C as *mut u32;
    const MPU_RASR: *mut u32 = 0xE000_EDA0 as *mut u32;
    unsafe {
        if rnr == gale::mpu_switch::MPU_CTRL_ID {
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

// Naked MemManage handler: on entry MSP points at the hardware-stacked frame
// {r0,r1,r2,r3,r12,lr,pc,xpsr}. Hand that pointer to the Rust recorder, which
// returns through the untouched EXC_RETURN in lr.
core::arch::global_asm!(
    ".section .text.MemoryManagement",
    ".global MemoryManagement",
    ".thumb_func",
    "MemoryManagement:",
    "    mrs r0, msp",
    "    b   priv_memmanage",
);

/// Record the fault, clear the sticky MemManage status, restore PRIVILEGE for
/// thread mode, and redirect the stacked PC to the armed continuation.
///
/// Clearing `CONTROL.nPRIV` here is what lets the probe report at all: the
/// faulting code is unprivileged and cannot write `CONTROL` itself, and the
/// write takes effect for thread mode on exception return.
#[no_mangle]
extern "C" fn priv_memmanage(frame: *mut u32) {
    unsafe {
        let cfsr = read_volatile(CFSR);
        let mmfar = read_volatile(MMFAR);
        write_volatile(addr_of_mut!(FAULT_CFSR), cfsr);
        write_volatile(addr_of_mut!(FAULT_MMFAR), mmfar);
        let n = read_volatile(addr_of!(FAULT_COUNT));
        write_volatile(addr_of_mut!(FAULT_COUNT), n + 1);
        write_volatile(CFSR, 0xFF);

        let ctrl: u32;
        core::arch::asm!("mrs {}, CONTROL", out(reg) ctrl);
        core::arch::asm!("msr CONTROL, {}", in(reg) ctrl & !1u32);

        let resume = read_volatile(addr_of!(RESUME_PC));
        if resume == 0 {
            write_volatile(MPU_CTRL, 0);
            cortex_m::asm::dsb();
            cortex_m::asm::isb();
            hprintln!(
                "gust-priv-region-probe FAIL: unexpected MemManage (no resume armed) pc={:#010x} CFSR={:#010x} MMFAR={:#010x}",
                read_volatile(frame.add(6)),
                cfsr,
                mmfar
            );
            debug::exit(debug::EXIT_FAILURE);
            loop {}
        }
        write_volatile(addr_of_mut!(RESUME_PC), 0);
        write_volatile(frame.add(6), resume & !1);
        let xpsr = read_volatile(frame.add(7));
        write_volatile(frame.add(7), (xpsr & !0x0600_FC00) | (1 << 24));
    }
}

// The unprivileged read, and the continuation the handler redirects to.
//
// Written in assembly so the ONE load under test is a single instruction at a
// known place, and so the resume target is an address this file can name. The
// read result goes to READ_BACK, which lives in the stack region (reachable
// unprivileged in both arms — only the TARGET region's permission differs).
core::arch::global_asm!(
    ".section .text.priv_probe_body",
    ".global priv_probe_body",
    ".global priv_probe_resume",
    ".thumb_func",
    "priv_probe_body:",          // r0 = target address, r1 = &READ_BACK
    "    ldr  r2, [r0]",         // THE instruction under test
    "    str  r2, [r1]",
    "priv_probe_resume:",        // handler redirects here on a fault
    "    svc  #0",               // back to privileged (see SVCall)
    "    bx   lr",
);

unsafe extern "C" {
    fn priv_probe_body(target: u32, sink: *mut u32);
    fn priv_probe_resume();
}

/// Regain thread-mode privilege on the SUCCESS path. Unprivileged code cannot
/// write CONTROL; an exception can, and the write applies to thread mode on
/// return. The fault path reaches privilege the same way, inside
/// `priv_memmanage`.
#[unsafe(no_mangle)]
extern "C" fn SVCall() {
    unsafe {
        let ctrl: u32;
        core::arch::asm!("mrs {}, CONTROL", out(reg) ctrl);
        core::arch::asm!("msr CONTROL, {}", in(reg) ctrl & !1u32);
    }
}

#[entry]
fn main() -> ! {
    let arm = if cfg!(feature = "priv-private") { "private" } else { "shared" };

    unsafe {
        // The model is proven against DREGION == 8. On a part with more, the
        // sequence leaves the extra slots stale and deny-by-default is defeated.
        let dregion = (read_volatile(MPU_TYPE) >> 8) & 0xFF;
        if dregion != REQUIRED_DREGION {
            hprintln!(
                "gust-priv-region-probe FAIL({}): MPU_TYPE.DREGION={} — the model is proven for {}",
                arm, dregion, REQUIRED_DREGION
            );
            debug::exit(debug::EXIT_FAILURE);
            loop {}
        }

        // Seed the target while privileged and unprotected, so a successful
        // unprivileged read has something to prove it actually read.
        write_volatile(MPU_CTRL, 0);
        cortex_m::asm::dsb();
        cortex_m::asm::isb();
        write_volatile(TARGET_BASE as *mut u32, MAGIC);

        // MemManage must be enabled, or the fault escalates to HardFault and
        // the arms stop being distinguishable.
        write_volatile(SHCSR, read_volatile(SHCSR) | (1 << 16));
        write_volatile(CFSR, 0xFF);
    }

    // The table, built through the VERIFIED builder — the whole point is that
    // the permission reaching hardware is the one the proofs are about.
    let mut t = RegionTable::new();
    let ok_code = t.try_add_region(0, CODE_BASE, CODE_SIZE, false);
    let ok_ram = t.try_add_region(0, RAM_BASE, RAM_SIZE, true);

    // THE ONE DIFFERENCE BETWEEN THE ARMS.
    #[cfg(feature = "priv-private")]
    let target_unpriv = UNPRIV_NONE;
    #[cfg(feature = "priv-shared")]
    let target_unpriv = UNPRIV_SAME;
    let ok_tgt = t.try_add_region_perm(0, TARGET_BASE, TARGET_SIZE, true, target_unpriv);

    if !(ok_code && ok_ram && ok_tgt) {
        hprintln!(
            "gust-priv-region-probe FAIL({}): builder rejected a grant (code={} ram={} target={}) — the experiment never ran",
            arm, ok_code, ok_ram, ok_tgt
        );
        debug::exit(debug::EXIT_FAILURE);
        loop {}
    }

    // Program the MPU through the verified sequence, then confirm it is on:
    // every assertion below is about an ENFORCING MPU.
    t.switch_to_partition(0);
    let ctrl = unsafe { read_volatile(MPU_CTRL) };
    if ctrl & MPU_CTRL_ENABLE == 0 {
        hprintln!("gust-priv-region-probe FAIL({}): MPU_CTRL={:#x} — not enabled", arm, ctrl);
        debug::exit(debug::EXIT_FAILURE);
        loop {}
    }

    unsafe {
        write_volatile(addr_of_mut!(RESUME_PC), priv_probe_resume as *const () as u32);
        // Drop to unprivileged thread mode, still on MSP (SPSEL untouched).
        let c: u32;
        core::arch::asm!("mrs {}, CONTROL", out(reg) c);
        core::arch::asm!("msr CONTROL, {}", in(reg) c | 1u32);
        cortex_m::asm::isb();

        priv_probe_body(TARGET_BASE, addr_of_mut!(READ_BACK));

        // Privileged again (via SVCall on the success path, or via the
        // MemManage handler on the fault path). Turn the MPU off before
        // reporting so the semihosting path is unconstrained.
        write_volatile(MPU_CTRL, 0);
        cortex_m::asm::dsb();
        cortex_m::asm::isb();
    }

    let faults = unsafe { read_volatile(addr_of!(FAULT_COUNT)) };
    let mmfar = unsafe { read_volatile(addr_of!(FAULT_MMFAR)) };
    let cfsr = unsafe { read_volatile(addr_of!(FAULT_CFSR)) };
    let got = unsafe { read_volatile(addr_of!(READ_BACK)) };

    #[cfg(feature = "priv-private")]
    {
        // MMARVALID (CFSR bit 7) must be set, or MMFAR carries no address and
        // "it faulted at the target" is not something this run observed.
        let mmarvalid = cfsr & (1 << 7) != 0;
        if faults == 1 && mmarvalid && mmfar == TARGET_BASE {
            hprintln!(
                "gust-priv-region-probe OK(private): unprivileged read of a UNPRIV_NONE region FAULTED — MemManage at {:#010x}, CFSR={:#010x}, read-back {:#010x} (unwritten)",
                mmfar, cfsr, got
            );
            debug::exit(debug::EXIT_SUCCESS);
        } else {
            hprintln!(
                "gust-priv-region-probe FAIL(private): want exactly 1 MemManage with MMARVALID and MMFAR={:#010x}; got faults={} CFSR={:#010x} MMFAR={:#010x} read-back={:#010x}",
                TARGET_BASE, faults, cfsr, mmfar, got
            );
            debug::exit(debug::EXIT_FAILURE);
        }
    }

    #[cfg(feature = "priv-shared")]
    {
        if faults == 0 && got == MAGIC {
            hprintln!(
                "gust-priv-region-probe OK(shared): unprivileged read of the SAME region marked UNPRIV_SAME SUCCEEDED — got {:#010x}, 0 faults. This is the control: the private arm's fault is the permission, not the mapping.",
                got
            );
            debug::exit(debug::EXIT_SUCCESS);
        } else {
            hprintln!(
                "gust-priv-region-probe FAIL(shared): want 0 faults and {:#010x}; got faults={} value={:#010x} CFSR={:#010x} MMFAR={:#010x}",
                MAGIC, faults, got, cfsr, mmfar
            );
            debug::exit(debug::EXIT_FAILURE);
        }
    }

    loop {}
}
