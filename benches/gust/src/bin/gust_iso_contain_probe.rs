//! gust-iso-contain-probe — THE v0.5.0 I-ISO flagship oracle: a REAL, archived
//! compiler miscompile (synth#757), physically CONTAINED by the verified MPU
//! region-programming core (gale src/mpu_switch.rs, Verus 1098/0 + Kani 4/4)
//! instead of silently corrupting output.
//!
//! SCOPE: this is FAULT-containment (an accidental compiler miscompile), NOT
//! security-containment. The tenant runs privileged and the PPB (MPU_CTRL) is
//! not MPU-checked, so a *malicious* privileged tenant could reprogram the MPU;
//! defeating that needs unprivileged tenants and is out of scope here. Evidence
//! is local qemu lm3s6965evb (real v7-M MPU / CFSR=0x82), not yet CI-gated.
//!
//! The tenant: drivers/os-node/repro-757/os-tl-buggy.o — the BUGGY synth
//! 0.45.0 dissolve of the archived exact miscompile input repro-757/loom.wasm
//! (md5 18da000d9142dfa0885f57578d3af150). Its ONLY difference from the fixed
//! 0.45.1 dissolve (repro-757/os-tl-fixed.o, byte-identical .text and .data)
//! is ONE relocation: the literal-pool word at .text+0x694 in func_20 (addend
//! +8, the log string-copy's head-chunk source pointer) is bound to
//! __synth_wasm_seg_0 instead of __synth_wasm_seg_2. Unlinked and un-MPU'd,
//! that stale read silently emits [2,0,0,0,1,0,0,32,...] instead of
//! "gust:os up\n" (verified live before this probe was built — see
//! repro-757/REPRO.md; gust_os_tl_probe FAILs with exactly that signature
//! when linked against the buggy object).
//!
//! Containment mechanism (see iso_contain.x for the full picture): build.rs
//! objcopy-renames the object's .data (seg_0 at +0x00, seg_1 +0x0c, seg_2
//! +0x18 — ONE section, so seg_0 cannot be split out alone) to
//! .iso_stale_data, and the linker pins it at 0x2000_BFF0, straddling this
//! probe's MPU guard boundary at 0x2000_C000:
//!
//!   * the miscompiled read's target seg_0+8 = [0x2000_BFF8, 0x2000_C000)
//!     falls inside the DENIED guard [0x2000_8000, 0x2000_C000);
//!   * everything the CORRECT program needs is granted: code (flash), the
//!     true string at seg_2+8 = 0x2000_C010, __synth_globals (+0x30 =
//!     0x2000_C020), the arena .bss + shadow stack (SRAM low), the stack.
//!
//! The MPU is programmed EXCLUSIVELY through the verified path
//! (RegionTable::switch_to_partition → program_partition [P1–P4 proven] →
//! apply_program → this probe's contract-faithful mpu_write seam: DSB+ISB
//! after MPU_CTRL writes, init-time MPU_TYPE.DREGION==8 refusal).
//!
//! Oracle: run() must MemManage-FAULT with the recorded MMFAR ==
//! __synth_wasm_seg_0+8 (the stale bind, physically denied) and ZERO bytes
//! reaching the log sink — the compiler bug is CONTAINED, not silent. The
//! no-fault control under the IDENTICAL memory arrangement is
//! gust_iso_contain_ctl (fixed object: runs clean, log correct).
//! Exit codes: semihosting EXIT_SUCCESS / EXIT_FAILURE.
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
const MPU_CTRL_ID: u32 = 0xFFFF_FFFF;

/// The MPU guard hole partition 0 grants to NOBODY (physically-backed SRAM).
const GUARD_START: u32 = 0x2000_8000;
const GUARD_END: u32 = 0x2000_C000;
/// Where iso_contain.x pins the renamed .data of the dissolved object.
const STALE_DATA_VMA: u32 = 0x2000_BFF0;

const UART_DR: u32 = 0x4001_3804; // USART1 DR (the gust:hal/mmio log sink)
const CAP: usize = 32;

static mut LOG_BUF: [u8; CAP] = [0; CAP];
static mut LOG_LEN: usize = 0;

// ---------------------------------------------------------------------------
// The dissolved node's TCB seam (same bridge as gust_os_tl_probe): one mmio
// read atom (timer CNT) and one mmio write atom (UART_DR log sink).
// ---------------------------------------------------------------------------
#[no_mangle]
pub extern "C" fn read32(_addr: u32) -> u32 {
    1000
}

#[no_mangle]
pub extern "C" fn write32(addr: u32, val: u32) {
    if addr == UART_DR {
        unsafe {
            let len = addr_of_mut!(LOG_LEN);
            if *len < CAP {
                let buf = addr_of_mut!(LOG_BUF) as *mut u8;
                *buf.add(*len) = val as u8;
                *len += 1;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The `mpu_write` trusted-seam bridge (contract item 1: DSB+ISB after every
// MPU_CTRL write) — identical to gust_iso_fault_probe's.
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

/// Contract item 2: refuse to start unless MPU_TYPE.DREGION == 8.
fn mpu_bridge_init_check() {
    let dregion = (unsafe { read_volatile(MPU_TYPE) } >> 8) & 0xFF;
    if dregion != REQUIRED_DREGION {
        hprintln!(
            "gust-iso-contain-probe FAIL: platform contract violated — MPU_TYPE.DREGION={} (require {}); refusing to start",
            dregion,
            REQUIRED_DREGION
        );
        debug::exit(debug::EXIT_FAILURE);
    }
}

// ---------------------------------------------------------------------------
// MemManage recording + resume-continuation redirect (as in the fault probe).
// ---------------------------------------------------------------------------
static mut FAULT_COUNT: u32 = 0;
static mut FAULT_MMFAR: u32 = 0;
static mut FAULT_CFSR: u32 = 0;
static mut RESUME_PC: u32 = 0;

core::arch::global_asm!(
    ".section .text.MemoryManagement",
    ".global MemoryManagement",
    ".thumb_func",
    "MemoryManagement:",
    "    mrs r0, msp",
    "    b   iso_memmanage",
);

#[no_mangle]
extern "C" fn iso_memmanage(frame: *mut u32) {
    unsafe {
        let cfsr = read_volatile(CFSR);
        let mmfar = read_volatile(MMFAR);
        write_volatile(addr_of_mut!(FAULT_CFSR), cfsr);
        write_volatile(addr_of_mut!(FAULT_MMFAR), mmfar);
        let n = read_volatile(addr_of!(FAULT_COUNT));
        write_volatile(addr_of_mut!(FAULT_COUNT), n + 1);
        write_volatile(CFSR, 0xFF);
        let resume = read_volatile(addr_of!(RESUME_PC));
        if resume == 0 {
            write_volatile(MPU_CTRL, 0);
            cortex_m::asm::dsb();
            cortex_m::asm::isb();
            hprintln!(
                "gust-iso-contain-probe FAIL: unexpected MemManage (no resume armed) pc={:#010x} CFSR={:#010x} MMFAR={:#010x}",
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

#[exception]
unsafe fn HardFault(ef: &cortex_m_rt::ExceptionFrame) -> ! {
    unsafe {
        write_volatile(MPU_CTRL, 0);
    }
    cortex_m::asm::dsb();
    cortex_m::asm::isb();
    hprintln!(
        "gust-iso-contain-probe FAIL: HardFault pc={:#010x} CFSR={:#010x} MMFAR={:#010x}",
        ef.pc(),
        unsafe { read_volatile(CFSR) },
        unsafe { read_volatile(MMFAR) }
    );
    debug::exit(debug::EXIT_FAILURE);
    loop {}
}

// ---------------------------------------------------------------------------
// The dissolved node: `run` behind the r11=0 trampoline (--native-pointer-abi
// pins r11 as the wasm linmem base — same pattern as gust_os_tl_probe).
// ---------------------------------------------------------------------------
core::arch::global_asm!(
    ".section .text.run_tl",
    ".global run_tl",
    ".thumb_func",
    "run_tl:",
    "    push  {{r11, lr}}",
    "    mov.w r11, #0",
    "    bl    run",
    "    pop   {{r11, pc}}",
);

extern "C" {
    fn run_tl() -> u32;
    // The dissolved object's segment symbols (now in .iso_stale_data).
    static __synth_wasm_seg_0: u8;
    static __synth_wasm_seg_2: u8;
    // iso_contain.x placement + load symbols.
    static __iso_stale_data_start: u8;
    static __iso_stale_data_end: u8;
    static __iso_stale_data_lma: u8;
}

/// The containment continuation: entered ONLY via the handler's stacked-PC
/// redirect when the tenant faults. Judges the oracle and exits.
#[no_mangle]
extern "C" fn contained() -> ! {
    let (n, mmfar, cfsr) = unsafe {
        (
            read_volatile(addr_of!(FAULT_COUNT)),
            read_volatile(addr_of!(FAULT_MMFAR)),
            read_volatile(addr_of!(FAULT_CFSR)),
        )
    };
    let stale_addr = addr_of!(__synth_wasm_seg_0) as u32 + 8;
    let log_len = unsafe { read_volatile(addr_of!(LOG_LEN)) };
    let ok = n == 1 && mmfar == stale_addr && cfsr & 0x82 == 0x82 && log_len == 0;
    if ok {
        hprintln!(
            "gust-iso-contain-probe OK: synth#757 miscompiled read DENIED @{:#010x} == __synth_wasm_seg_0+8 (CFSR={:#010x} DACCVIOL+MMARVALID), 0 bytes reached the log sink — a real compiler bug PHYSICALLY CONTAINED by the verified I-ISO MPU core",
            mmfar,
            cfsr
        );
        debug::exit(debug::EXIT_SUCCESS);
    } else {
        hprintln!(
            "gust-iso-contain-probe FAIL: fault recorded but wrong shape (faults={} MMFAR={:#010x} want {:#010x} CFSR={:#010x} log_len={})",
            n,
            mmfar,
            stale_addr,
            cfsr,
            log_len
        );
        debug::exit(debug::EXIT_FAILURE);
    }
    loop {}
}

#[entry]
fn main() -> ! {
    mpu_bridge_init_check();
    unsafe {
        write_volatile(SHCSR, read_volatile(SHCSR) | (1 << 16));
    }

    // Initialise .iso_stale_data (VMA 0x2000_BFF0 <- FLASH LMA): cortex-m-rt
    // only copies its own .data. MPU is still off here.
    unsafe {
        let start = addr_of!(__iso_stale_data_start) as *mut u8;
        let end = addr_of!(__iso_stale_data_end) as *const u8;
        let lma = addr_of!(__iso_stale_data_lma) as *const u8;
        let n = end as usize - start as usize;
        for i in 0..n {
            write_volatile(start.add(i), read_volatile(lma.add(i)));
        }
    }

    // Placement pre-flight (MPU off): the mechanism this oracle rests on must
    // hold BEFORE we claim anything about enforcement.
    let seg0 = addr_of!(__synth_wasm_seg_0) as u32;
    let seg2 = addr_of!(__synth_wasm_seg_2) as u32;
    let stale = seg0 + 8; // the miscompiled literal: seg_0 + addend 8
    let placement_ok = seg0 == STALE_DATA_VMA
        && stale >= GUARD_START
        && stale + 8 <= GUARD_END          // whole 8-byte head chunk denied
        && seg2 + 8 >= GUARD_END;          // correct string fully granted
    let string_ok = unsafe {
        let want: &[u8] = b"gust:os up\n";
        let src = (seg2 + 8) as *const u8;
        (0..want.len()).all(|i| read_volatile(src.add(i)) == want[i])
    };
    if !placement_ok || !string_ok {
        hprintln!(
            "gust-iso-contain-probe FAIL: placement pre-flight (seg_0={:#010x} want {:#010x}, stale={:#010x} must be in [{:#010x},{:#010x}), seg_2={:#010x}, string_ok={})",
            seg0,
            STALE_DATA_VMA,
            stale,
            GUARD_START,
            GUARD_END,
            seg2,
            string_ok
        );
        debug::exit(debug::EXIT_FAILURE);
    }

    // Partition 0 — the same deny-by-default map as gust_iso_fault_probe,
    // programmed via the VERIFIED core only:
    //   r0 flash 256K RO+X | r1 SRAM-low 32K RW | r2 stack window 16K RW
    //   guard hole [0x2000_8000, 0x2000_C000) granted to nobody (P2 emits
    //   slots 3..7 DISABLED).
    let mut t = RegionTable::new();
    t.base[0] = 0x0000_0000;
    t.size[0] = 0x0004_0000;
    t.enabled[0] = true;
    t.writable[0] = false;
    t.base[1] = 0x2000_0000;
    t.size[1] = 0x0000_8000;
    t.enabled[1] = true;
    t.writable[1] = true;
    t.base[2] = 0x2000_C000;
    t.size[2] = 0x0000_4000;
    t.enabled[2] = true;
    t.writable[2] = true;
    t.switch_to_partition(0);

    let ctrl = unsafe { read_volatile(MPU_CTRL) };
    if ctrl != MPU_CTRL_ENABLE {
        hprintln!(
            "gust-iso-contain-probe FAIL: MPU_CTRL={:#x} after verified switch (want {:#x})",
            ctrl,
            MPU_CTRL_ENABLE
        );
        debug::exit(debug::EXIT_FAILURE);
    }

    // Arm the containment continuation and run the miscompiled tenant.
    unsafe {
        write_volatile(addr_of_mut!(RESUME_PC), contained as *const () as u32);
    }
    compiler_fence(Ordering::SeqCst);
    let r = unsafe { run_tl() };
    compiler_fence(Ordering::SeqCst);

    // run() returned: the stale read was NOT trapped — the bug escaped.
    unsafe {
        write_volatile(MPU_CTRL, 0);
    }
    cortex_m::asm::dsb();
    cortex_m::asm::isb();
    let (len, captured) = unsafe { (read_volatile(addr_of!(LOG_LEN)), *addr_of!(LOG_BUF)) };
    hprintln!(
        "gust-iso-contain-probe FAIL: run() returned {:#x} without faulting — miscompile NOT contained (log_len={} log_bytes={:?})",
        r,
        len,
        &captured[..len]
    );
    debug::exit(debug::EXIT_FAILURE);
    loop {}
}
