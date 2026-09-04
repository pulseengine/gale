//! gust-iso-unpriv-probe — REQ-OS-UNPRIV-001's gap, made EXECUTABLE.
//!
//! VER-OS-ISO-001 records gale's isolation scope in prose: "fault-containment, not
//! security-containment — the demo tenant runs privileged and the PPB is not
//! MPU-checked". That sentence is correct, and until this probe it was the only
//! form the limitation had. A limitation that exists only as prose is one nobody
//! trips over: REQ-OS-MULTITENANT-001 asks for MUTUALLY-DISTRUSTING tenants, which
//! is a security-containment claim, and nothing connected the two.
//!
//! This probe asserts THE CURRENT STATE, not the desired one. Today a tenant runs
//! privileged, so it can clear MPU_CTRL.ENABLE and walk into a region the verified
//! table denied it. That is what this demonstrates, and CI gates the demonstration.
//!
//! POLARITY, deliberately. It exits SUCCESS when the escape WORKS. That reads
//! backwards until you see what it is for: it is a ledger with teeth, the same
//! shape check-driver-components.py uses for dma-own's raw env atoms — record the
//! actual state as an assertion so a change cannot pass silently. When
//! REQ-OS-UNPRIV-001 lands and tenant code runs unprivileged, the MPU_CTRL write
//! will fault, this probe will FAIL, and the failure is the signal to update
//! VER-OS-ISO-001's scope note and this file together. A green probe means the gap
//! is still open; a red one means somebody closed it and the paperwork is stale.
//!
//! What it does NOT claim: nothing here says the escape is exploitable in a
//! deployed gust image, or that any tenant does this. It says the hardware
//! configuration permits it, which is exactly what "the PPB is not MPU-checked"
//! means and exactly what a mutual-distrust claim cannot tolerate.
#![no_std]
#![no_main]

use core::ptr::{read_volatile, write_volatile};
use cortex_m_rt::{entry, exception};
use cortex_m_semihosting::{debug, hprintln};
use gale::mpu_switch::{RegionTable, MPU_CTRL_ENABLE, MPU_CTRL_ID, REQUIRED_DREGION};
use panic_halt as _;

const MPU_TYPE: *mut u32 = 0xE000_ED90 as *mut u32;
const MPU_CTRL: *mut u32 = 0xE000_ED94 as *mut u32;
const MPU_RNR: *mut u32 = 0xE000_ED98 as *mut u32;
const MPU_RBAR: *mut u32 = 0xE000_ED9C as *mut u32;
const MPU_RASR: *mut u32 = 0xE000_EDA0 as *mut u32;
const SHCSR: *mut u32 = 0xE000_ED24 as *mut u32;

/// Platform implementation of the verified core's trusted `mpu_write` seam,
/// identical to gust_iso_fault_probe's and carrying the same contract item 1
/// (DSB+ISB after every MPU_CTRL write, so the proven ordering reaches hardware).
///
/// That this probe would not LINK without it is the point REQ-OS-MPUSHIP-001
/// makes to a downstream: the verified core ships with exactly one undefined
/// symbol, and every consumer must supply it. `rust-lld: error: undefined symbol:
/// mpu_write` is the embedder obligation stated by the linker.
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

/// Physically-backed SRAM the verified table grants to NOBODY (same hole the
/// fault probe uses, so the two agree on what "denied" means).
const DENIED_ADDR: u32 = 0x2000_8000;

/// A MemManage here is the FUTURE state, not this probe's expected one: it means
/// the tenant could not reach the PPB, i.e. REQ-OS-UNPRIV-001 has landed.
#[exception]
unsafe fn MemoryManagement() -> ! {
    hprintln!(
        "gust-iso-unpriv-probe FAIL(stale-ledger): the tenant could NOT disable the MPU. \
         That is the DESIRED state — REQ-OS-UNPRIV-001 appears to have landed. Update \
         VER-OS-ISO-001's scope note (it still says security-containment is absent) and \
         retire or invert this probe."
    );
    debug::exit(debug::EXIT_FAILURE);
    loop {}
}

#[entry]
fn main() -> ! {
    // Platform contract item 2, as the fault probe does: refuse to start on a part
    // whose region count is not the 8 the verified core assumes.
    let dregion = (unsafe { read_volatile(MPU_TYPE) } >> 8) & 0xFF;
    if dregion != REQUIRED_DREGION {
        hprintln!(
            "gust-iso-unpriv-probe FAIL: MPU_TYPE.DREGION={}, need {}",
            dregion, REQUIRED_DREGION
        );
        debug::exit(debug::EXIT_FAILURE);
        loop {}
    }
    unsafe { write_volatile(SHCSR, read_volatile(SHCSR) | (1 << 16)) };

    // Program the same deny-by-default map through the VERIFIED path — no
    // hand-programming, so the escape is measured against the real table.
    let mut t = RegionTable::new();
    t.base[0] = 0x0000_0000; t.size[0] = 0x0004_0000; t.enabled[0] = true; t.writable[0] = false;
    t.base[1] = 0x2000_0000; t.size[1] = 0x0000_8000; t.enabled[1] = true; t.writable[1] = true;
    t.base[2] = 0x2000_C000; t.size[2] = 0x0000_4000; t.enabled[2] = true; t.writable[2] = true;
    t.switch_to_partition(0);

    let armed = unsafe { read_volatile(MPU_CTRL) };
    if armed & MPU_CTRL_ENABLE == 0 {
        hprintln!("gust-iso-unpriv-probe FAIL: MPU never armed (CTRL={:#010x})", armed);
        debug::exit(debug::EXIT_FAILURE);
        loop {}
    }

    // --- the escape, performed as "tenant" code -----------------------------
    // On a Cortex-M the System Control Space is never MPU-checked. Tenant code
    // running privileged can therefore simply switch enforcement off. If
    // REQ-OS-UNPRIV-001 had landed this write would MemManage-fault.
    unsafe {
        write_volatile(MPU_CTRL, 0);
        cortex_m::asm::dsb();
        cortex_m::asm::isb();
    }
    let after = unsafe { read_volatile(MPU_CTRL) };

    // With enforcement off, the address the verified table denied is reachable.
    unsafe { write_volatile(DENIED_ADDR as *mut u32, 0xC0FF_EE00) };
    let readback = unsafe { read_volatile(DENIED_ADDR as *const u32) };

    let escaped = (after & MPU_CTRL_ENABLE == 0) && readback == 0xC0FF_EE00;
    if escaped {
        hprintln!(
            "gust-iso-unpriv-probe OK(gap-open): privileged tenant cleared MPU_CTRL \
             ({:#010x} -> {:#010x}) and wrote the DENIED address {:#010x}, read back \
             {:#010x}. Fault-containment holds; SECURITY-containment does not. \
             REQ-OS-UNPRIV-001 is open — this is the evidence, not a defect report.",
            armed, after, DENIED_ADDR, readback
        );
        debug::exit(debug::EXIT_SUCCESS);
    } else {
        hprintln!(
            "gust-iso-unpriv-probe FAIL: escape did not complete as recorded \
             (CTRL {:#010x} -> {:#010x}, readback {:#010x}). The ledger no longer \
             matches the hardware; do not assume either state.",
            armed, after, readback
        );
        debug::exit(debug::EXIT_FAILURE);
    }
    loop {}
}
