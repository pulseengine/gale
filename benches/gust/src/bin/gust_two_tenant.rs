//! REQ-OS-MPU-001's kill-criterion, executed: two tenants, one MPU, one crafted escape.
//!
//! THE CRITERION (synth#1145, adopted verbatim upstream): "a two-tenant image where
//! tenant A writes outside its region and the write lands in tenant B's memory instead
//! of faulting". So the image is arranged for that write to be POSSIBLE — memory 1 is
//! placed immediately above memory 0, meaning an overflow out of A lands squarely in B
//! unless the MPU stops it. A layout where the escape hits unmapped space would prove
//! far less.
//!
//! TENANTS RUN UNPRIVILEGED, and that is not a detail. gale measured that Renode
//! enforces the v7-M MPU but ignores MPU_CTRL.PRIVDEFENA: a PRIVILEGED access matching
//! no enabled region is silently granted the ARMv7-M default map. A privileged
//! demonstrator would therefore pass under Renode while proving nothing, and would
//! diverge from silicon in the reassuring direction. CONTROL.nPRIV=1 before the escape.
//!
//! REPORTING is magic words at fixed addresses inside the always-granted low region, not
//! semihosting: the Renode portable captures no SemihostingUart output headless. Read
//! back with `sysbus ReadDoubleWord` after `RunFor`.
//!
//! NEGATIVE CONTROL: feature `no-regions` skips the region programming entirely. The
//! escape must then LAND in tenant B and be observed doing so. Without that arm a fault
//! proves the platform rather than the programming, which is the defect this whole sweep
//! has been removing.
#![no_std]
#![no_main]

use core::ptr::{read_volatile, write_volatile};
use cortex_m_rt::{entry, exception, ExceptionFrame};
use gale::mpu_switch::{RegionTable, MPU_CTRL_ENABLE, MPU_CTRL_ID, REQUIRED_DREGION};
use panic_halt as _;

// ---- synth's region table (#1145). SHN_ABS symbols: the ADDRESS is the value. ----
unsafe extern "C" {
    static __synth_mem_count: u8;
    static __synth_mem_size_0: u8;
    static __synth_mem_size_1: u8;
    static __synth_mem_base_1: u8;
    // Tenant entry points, exported by the dissolved two-tenant module.
    fn a_store(addr: u32, val: u32);
    fn a_load(addr: u32) -> u32;
    fn b_load(addr: u32) -> u32;
    fn a_escape(addr: u32, val: u32);
}
#[inline(always)]
fn absval(s: &'static u8) -> u32 { core::ptr::from_ref(s) as u32 }

const MPU_TYPE: *mut u32 = 0xE000_ED90 as *mut u32;
const MPU_CTRL: *mut u32 = 0xE000_ED94 as *mut u32;
const MPU_RNR: *mut u32 = 0xE000_ED98 as *mut u32;
const MPU_RBAR: *mut u32 = 0xE000_ED9C as *mut u32;
const MPU_RASR: *mut u32 = 0xE000_EDA0 as *mut u32;
const SHCSR: *mut u32 = 0xE000_ED24 as *mut u32;

// Magic-word outputs. NOT at hardcoded addresses: the first version put them at
// 0x20000100, which is exactly where the linker placed MEM0 — the report and tenant A's
// memory were the same bytes. Renode resolves REPORT by symbol instead, so the linker
// stays the single authority on layout.
#[unsafe(no_mangle)]
static mut REPORT: [u32; 5] = [0; 5];
#[inline(always)]
fn slot(i: usize) -> *mut u32 { unsafe { (&raw mut REPORT as *mut u32).add(i) } }

const STEP_PROGRAMMED: u32 = 0x51AE_0010;
const STEP_BENIGN_OK: u32 = 0x51AE_0011;
const STEP_ESCAPE_ISSUED: u32 = 0x51AE_0012;

const R_CONTAINED: u32 = 0x600D_1145; // escape faulted -> criterion HELD
const R_ESCAPED: u32 = 0xBADD_1145;   // escape landed in B -> criterion VIOLATED
const R_NO_MPU: u32 = 0xBAD0_D4E6;
const R_BENIGN_LOST: u32 = 0xBAD0_1057;

/// Memory 0. Its base goes in R11 and its region is programmed from it.
///
/// 64 KiB-ALIGNED, and that is an MPU requirement rather than a preference. PMSAv7
/// requires a region's base to be aligned to its size, so a 64 KiB region must start on
/// a 64 KiB boundary. The first version of this file used a plain static, the linker put
/// it at 0x20000100, and `switch_to_partition` correctly refused to arm — the run
/// reported NO_MPU and I initially read that as "the platform has no MPU".
///
/// synth's ABI states this as the embedder's job: it emits `.synth.wasm_mem_k` with
/// `sh_addralign = 4` and "does NOT pre-align the section for your MPU, because it cannot
/// know which MPU you have ... your linker script owns this".
#[repr(align(65536))]
struct Aligned64K([u8; 65536]);
#[unsafe(no_mangle)]
static mut MEM0_ALIGNED: Aligned64K = Aligned64K([0; 65536]);
/// Globals table base (R9). Nothing in this module uses globals; it must still be valid.
#[unsafe(no_mangle)]
static mut GLOBALS: [u8; 256] = [0; 256];

fn out(p: *mut u32, v: u32) { unsafe { write_volatile(p, v) } }

/// The verified core's trusted seam, per its documented platform contract.
#[unsafe(no_mangle)]
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

/// Any fault after the escape is issued means the MPU contained it.
macro_rules! contained {
    () => {{
        if unsafe { read_volatile(slot(1)) } == STEP_ESCAPE_ISSUED {
            out(slot(0), R_CONTAINED);
        }
        loop {}
    }};
}
#[exception]
unsafe fn MemoryManagement() -> ! { contained!() }
#[exception]
unsafe fn BusFault() -> ! { contained!() }
#[exception]
unsafe fn HardFault(_e: &ExceptionFrame) -> ! { contained!() }

#[entry]
fn main() -> ! {
    let dregion = (unsafe { read_volatile(MPU_TYPE) } >> 8) & 0xFF;
    if dregion != REQUIRED_DREGION {
        out(slot(0), R_NO_MPU);
        loop {}
    }
    unsafe { write_volatile(SHCSR, read_volatile(SHCSR) | (1 << 16) | (1 << 17) | (1 << 18)) };

    let mem0 = &raw const MEM0_ALIGNED as u32;
    let mem1 = absval(unsafe { &__synth_mem_base_1 });
    let size0 = absval(unsafe { &__synth_mem_size_0 });
    let size1 = absval(unsafe { &__synth_mem_size_1 });
    out(slot(2), mem0);
    out(slot(3), mem1);

    // R11 = memory 0 base, R10 = its size, R9 = globals base. Before any export runs.
    unsafe {
        let g = &raw const GLOBALS as u32;
        core::arch::asm!("mov r11, {}", in(reg) mem0, options(nomem, nostack));
        core::arch::asm!("mov r10, {}", in(reg) size0, options(nomem, nostack));
        core::arch::asm!("mov r9,  {}", in(reg) g,     options(nomem, nostack));
    }

    // One region per memory, through the VERIFIED programmer. Region 0 is derived from
    // the same address loaded into R11 -- synth's one-source-of-truth requirement.
    #[cfg(not(feature = "no-regions"))]
    {
        let mut t = RegionTable::new();
        t.base[0] = 0x0000_0000; t.size[0] = 0x0004_0000; t.enabled[0] = true; t.writable[0] = false;
        t.base[1] = mem0;        t.size[1] = size0;       t.enabled[1] = true; t.writable[1] = true;
        t.base[2] = mem1;        t.size[2] = size1;       t.enabled[2] = true; t.writable[2] = true;
        t.switch_to_partition(0);
        if unsafe { read_volatile(MPU_CTRL) } & MPU_CTRL_ENABLE == 0 {
            out(slot(0), R_NO_MPU);
            loop {}
        }
    }
    out(slot(1), STEP_PROGRAMMED);

    // Benign: tenant A inside its own memory must still work.
    unsafe { a_store(0x40, 0xA1A1_A1A1) };
    if unsafe { a_load(0x40) } != 0xA1A1_A1A1 {
        out(slot(0), R_BENIGN_LOST);
        loop {}
    }
    out(slot(1), STEP_BENIGN_OK);

    // Unprivileged BEFORE the escape: Renode grants a privileged background access the
    // ARMv7-M default map regardless of PRIVDEFENA, so a privileged escape would be
    // permitted there and faulted on silicon -- the wrong way round for a gate.
    unsafe {
        let c: u32;
        core::arch::asm!("mrs {}, CONTROL", out(reg) c);
        core::arch::asm!("msr CONTROL, {}", in(reg) c | 1);
        cortex_m::asm::isb();
    }

    // THE CRIMINAL: tenant A writes one word past its own memory -- which is where
    // tenant B begins.
    out(slot(1), STEP_ESCAPE_ISSUED);
    unsafe { a_escape(size0, 0xBADD_BADD) };

    // Reached only if nothing faulted. Did it land in B?
    let landed = unsafe { b_load(0) };
    out(slot(4), landed);
    out(slot(0), R_ESCAPED);
    loop {}
}
