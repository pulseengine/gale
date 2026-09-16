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
use cortex_m_rt::{entry, exception, ExceptionFrame};
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

// The wohl.local boards (gale#397) take their memory map from the GENERATED target
// constants — measured on each board — instead of a hand-written per-board cfg arm.
#[cfg(feature = "target-wl55jc")]
#[path = "../../targets/generated/gust_target_stm32wl55.rs"]
#[allow(dead_code)]
mod target;
#[cfg(feature = "target-wb55rg")]
#[path = "../../targets/generated/gust_target_stm32wb55.rs"]
#[allow(dead_code)]
mod target;
#[cfg(feature = "target-g031k8")]
#[path = "../../targets/generated/gust_target_stm32g031.rs"]
#[allow(dead_code)]
mod target;
const BOARD_MAP: bool = cfg!(any(feature = "target-wl55jc", feature = "target-wb55rg", feature = "target-g031k8"));

/// ARMv6-M has no MemManage, BusFault, UsageFault or SHCSR enable bits: every fault
/// is a HardFault. The NUCLEO-G031K8 is the only such board here.
const ARMV6M: bool = cfg!(feature = "target-g031k8");

/// Physically-backed SRAM the verified table grants to NOBODY (same hole the
/// fault probe uses, so the two agree on what "denied" means).
#[cfg(not(any(feature = "target-wl55jc", feature = "target-wb55rg", feature = "target-g031k8")))]
const DENIED_ADDR: u32 = 0x2000_8000;
/// On a board map: the first byte past the low data window (see the map in main).
#[cfg(any(feature = "target-wl55jc", feature = "target-wb55rg", feature = "target-g031k8"))]
const DENIED_ADDR: u32 = target::SRAM_BASE + low_window(target::SRAM_LEN);

/// Low data window: half of SRAM, at most 32K. Stack window: a quarter, at most 16K,
/// ending at the top of SRAM (cortex-m-rt's initial SP). Both are powers of two and
/// aligned to their own size, as the MPU requires, for every SRAM length that is a
/// multiple of the stack window (8K, 64K, 96K, 192K all are).
#[allow(dead_code)]
const fn low_window(sram_len: u32) -> u32 { if sram_len / 2 < 0x8000 { sram_len / 2 } else { 0x8000 } }
#[allow(dead_code)]
const fn stack_window(sram_len: u32) -> u32 { if sram_len / 4 < 0x4000 { sram_len / 4 } else { 0x4000 } }

/// Set for exactly the instructions of the escape. On ARMv6-M every fault is a
/// HardFault, so "a fault happened" cannot tell the unprivileged-PPB fault this probe
/// is looking for apart from, say, a stack overflow in semihosting formatting. A
/// fault outside this window is reported as what it is, never as the mechanism.
static mut IN_ESCAPE: u32 = 0;

/// Under `drop-priv` a fault here is the DESIRED outcome: the tenant was
/// unprivileged and could not reach the PPB. Without it, a fault means somebody
/// closed the gap and this ledger is stale.
///
/// Which fault fires is MEASURED, not assumed. ARMv7-M routes an unprivileged
/// System Control Space access to a BusFault rather than a MemManage, and it
/// escalates to HardFault if BusFault is not enabled -- so all three are handled
/// and each reports itself by name. Guessing one and catching nothing would look
/// exactly like "no fault happened", which is the wrong conclusion.
macro_rules! blocked {
    ($which:expr) => {{
        if unsafe { read_volatile(core::ptr::addr_of!(IN_ESCAPE)) } != 1 {
            hprintln!(
                "gust-iso-unpriv-probe FAIL(unexpected-fault): {} OUTSIDE the escape \
                 window. This says nothing about the MPU; the probe itself broke.",
                $which
            );
            debug::exit(debug::EXIT_FAILURE);
            loop {}
        }
        #[cfg(feature = "drop-priv")]
        {
            hprintln!(
                "gust-iso-unpriv-probe OK(mechanism-works): unprivileged tenant could NOT \
                 write MPU_CTRL -- {} raised. Dropping to CONTROL.nPRIV=1 blocks the PPB \
                 escape, so REQ-OS-UNPRIV-001's mechanism is available on this core.",
                $which
            );
            debug::exit(debug::EXIT_SUCCESS);
        }
        #[cfg(not(feature = "drop-priv"))]
        {
            hprintln!(
                "gust-iso-unpriv-probe FAIL(stale-ledger): a PRIVILEGED tenant could not \
                 write MPU_CTRL -- {} raised. That is the DESIRED state; REQ-OS-UNPRIV-001 \
                 appears to have landed. Update VER-OS-ISO-001's scope note and this probe.",
                $which
            );
            debug::exit(debug::EXIT_FAILURE);
        }
        loop {}
    }};
}

#[cfg(not(feature = "target-g031k8"))]
#[exception]
unsafe fn MemoryManagement() -> ! {
    blocked!("MemManage")
}

#[cfg(not(feature = "target-g031k8"))]
#[exception]
unsafe fn BusFault() -> ! {
    blocked!("BusFault")
}

#[exception]
unsafe fn HardFault(_ef: &ExceptionFrame) -> ! {
    blocked!("HardFault (escalated)")
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
    // Enable MemManage (16), BusFault (17) AND UsageFault (18). Enabling only
    // MemManage was a qemu-shaped assumption: qemu's cortex-m3 answers an
    // unprivileged PPB write with a MemManage, so one bit was enough there. The
    // STM32G474 answers with a BUSFAULT -- "Precise data access error at location
    // 0xe000ed94" (MPU_CTRL) -- which with BUSFAULTENA clear escalates straight to
    // HardFault, and the debugger catches that before any handler of ours reports
    // anything. Same protection, different exception; the probe must be able to
    // observe either or it will mistake a caught HardFault for a crash.
    if !ARMV6M {
        unsafe { write_volatile(SHCSR, read_volatile(SHCSR) | (1 << 16) | (1 << 17) | (1 << 18)) };
    }

    // Program the same deny-by-default map through the VERIFIED path — no
    // hand-programming, so the escape is measured against the real table.
    let mut t = RegionTable::new();
    // The map is PER-BOARD, and finding that out took real silicon. The qemu
    // lm3s6965evb map below puts flash at 0x0000_0000; the STM32G474 puts it at
    // 0x0800_0000, so on the G474 the first version of this probe granted a range
    // the code was not in and died in memcpy before reaching the escape:
    //
    //   HardFault <Cause: Escalated MemManage Fault <Cause: Derived fault on
    //   exception entry>> in compiler_builtins::mem::memcpy
    //
    // "Derived fault on exception entry" means the handler could not even be
    // entered. A deny-by-default MPU is unforgiving about a map that does not
    // describe the part, which is the whole point of it.
    #[cfg(not(any(feature = "silicon-g474", feature = "target-wl55jc", feature = "target-wb55rg", feature = "target-g031k8")))]
    {
        // qemu lm3s6965evb: FLASH 0x0000_0000 256K, RAM 0x2000_0000 64K.
        t.base[0] = 0x0000_0000; t.size[0] = 0x0004_0000; t.enabled[0] = true; t.writable[0] = false;
        t.base[1] = 0x2000_0000; t.size[1] = 0x0000_8000; t.enabled[1] = true; t.writable[1] = true;
        t.base[2] = 0x2000_C000; t.size[2] = 0x0000_4000; t.enabled[2] = true; t.writable[2] = true;
    }
    #[cfg(feature = "silicon-g474")]
    {
        // NUCLEO-G474RE: FLASH 0x0800_0000 512K, RAM 0x2000_0000 96K (stack top
        // 0x2001_8000). Region 2 is the 16K window the stack actually lives in --
        // 0x2001_4000 is 16K-aligned, as the MPU requires.
        t.base[0] = 0x0800_0000; t.size[0] = 0x0008_0000; t.enabled[0] = true; t.writable[0] = false;
        t.base[1] = 0x2000_0000; t.size[1] = 0x0000_8000; t.enabled[1] = true; t.writable[1] = true;
        t.base[2] = 0x2001_4000; t.size[2] = 0x0000_4000; t.enabled[2] = true; t.writable[2] = true;
    }
    #[cfg(any(feature = "target-wl55jc", feature = "target-wb55rg", feature = "target-g031k8"))]
    {
        // Generated from the board's measured model. Flash is granted whole (its
        // length is a power of two on every board here); SRAM is granted as a low
        // data window and a top stack window, leaving DENIED_ADDR between them
        // physically backed and granted to nobody.
        use target::{FLASH_BASE, FLASH_LEN, SRAM_BASE, SRAM_LEN};
        let (lo, st) = (low_window(SRAM_LEN), stack_window(SRAM_LEN));
        t.base[0] = FLASH_BASE; t.size[0] = FLASH_LEN; t.enabled[0] = true; t.writable[0] = false;
        t.base[1] = SRAM_BASE; t.size[1] = lo; t.enabled[1] = true; t.writable[1] = true;
        t.base[2] = SRAM_BASE + SRAM_LEN - st; t.size[2] = st; t.enabled[2] = true; t.writable[2] = true;
        if DENIED_ADDR + 4 > t.base[2] {
            hprintln!("gust-iso-unpriv-probe FAIL: no denied gap between the data and stack windows");
            debug::exit(debug::EXIT_FAILURE);
            loop {}
        }
    }
    let _ = BOARD_MAP;
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
    // Under `drop-priv`, become unprivileged FIRST. CONTROL.nPRIV=1 with an ISB
    // so the change is in effect before the next instruction is fetched.
    #[cfg(feature = "drop-priv")]
    unsafe {
        let ctrl: u32;
        core::arch::asm!("mrs {}, CONTROL", out(reg) ctrl);
        core::arch::asm!("msr CONTROL, {}", in(reg) ctrl | 1);
        cortex_m::asm::isb();
        // CONTROL.nPRIV is OPTIONAL on ARMv6-M (the unprivileged/privileged
        // extension). If it did not stick, the "escape succeeded" below would be
        // reported as the mechanism failing, when the core simply has no mechanism.
        let now: u32;
        core::arch::asm!("mrs {}, CONTROL", out(reg) now);
        if now & 1 == 0 {
            hprintln!(
                "gust-iso-unpriv-probe FAIL(no-npriv): CONTROL.nPRIV reads 0 after setting it \
                 — this core has no unprivileged mode, so REQ-OS-UNPRIV-001's mechanism is \
                 unavailable here by hardware."
            );
            debug::exit(debug::EXIT_FAILURE);
            loop {}
        }
    }
    unsafe {
        write_volatile(core::ptr::addr_of_mut!(IN_ESCAPE), 1);
        write_volatile(MPU_CTRL, 0);
        cortex_m::asm::dsb();
        cortex_m::asm::isb();
    }
    let after = unsafe { read_volatile(MPU_CTRL) };

    // With enforcement off, the address the verified table denied is reachable.
    unsafe { write_volatile(DENIED_ADDR as *mut u32, 0xC0FF_EE00) };
    let readback = unsafe { read_volatile(DENIED_ADDR as *const u32) };
    unsafe { write_volatile(core::ptr::addr_of_mut!(IN_ESCAPE), 0) };

    let escaped = (after & MPU_CTRL_ENABLE == 0) && readback == 0xC0FF_EE00;
    #[cfg(feature = "drop-priv")]
    {
        hprintln!(
            "gust-iso-unpriv-probe FAIL(mechanism-absent): unprivileged tenant STILL \
             disabled the MPU (CTRL {:#010x} -> {:#010x}, readback {:#010x}). No fault \
             was raised, so CONTROL.nPRIV does not protect the PPB on this core and \
             REQ-OS-UNPRIV-001 needs a different mechanism.",
            armed, after, readback
        );
        debug::exit(debug::EXIT_FAILURE);
        loop {}
    }
    #[cfg(not(feature = "drop-priv"))]
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
