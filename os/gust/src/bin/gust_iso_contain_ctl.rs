//! gust-iso-contain-ctl — the NO-FAULT CONTROL for gust_iso_contain_probe:
//! the FIXED synth 0.45.1 dissolve of the same archived synth#757 input
//! (repro-757/os-tl-fixed.o — byte-identical .text/.data to the buggy object;
//! the ONLY difference is the one .text+0x694 relocation, correctly bound to
//! __synth_wasm_seg_2) linked under the IDENTICAL memory arrangement:
//! same .data→.iso_stale_data rename, same 0x2000_BFF0 straddle placement
//! (iso_contain.x), same verified-core MPU map (flash RO / SRAM-low RW /
//! stack-window RW, guard hole [0x2000_8000, 0x2000_C000) denied).
//!
//! This control discharges the objection "your straddle placement faults ANY
//! tenant": with the single relocation fixed, every address the program
//! actually touches is granted, so run() must complete WITHOUT any MemManage,
//! return 0xA, and emit the correct "gust:os up\n" log. Probe FAULTS + ctl
//! RUNS CLEAN together pin the containment to exactly the miscompiled read.
//! Exit codes: semihosting EXIT_SUCCESS / EXIT_FAILURE.
#![no_std]
#![no_main]

use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use cortex_m_rt::{entry, exception};
use cortex_m_semihosting::{debug, hprintln};
use gale::mpu_switch::{RegionTable, MPU_CTRL_ENABLE, REQUIRED_DREGION};
use panic_halt as _;

const MPU_TYPE: *mut u32 = 0xE000_ED90 as *mut u32;
const MPU_CTRL: *mut u32 = 0xE000_ED94 as *mut u32;
const MPU_RNR: *mut u32 = 0xE000_ED98 as *mut u32;
const MPU_RBAR: *mut u32 = 0xE000_ED9C as *mut u32;
const MPU_RASR: *mut u32 = 0xE000_EDA0 as *mut u32;
const SHCSR: *mut u32 = 0xE000_ED24 as *mut u32;
const CFSR: *mut u32 = 0xE000_ED28 as *mut u32;
const MMFAR: *mut u32 = 0xE000_ED34 as *mut u32;
const MPU_CTRL_ID: u32 = 0xFFFF_FFFF;

const STALE_DATA_VMA: u32 = 0x2000_BFF0;
const UART_DR: u32 = 0x4001_3804;
const CAP: usize = 32;

static mut LOG_BUF: [u8; CAP] = [0; CAP];
static mut LOG_LEN: usize = 0;

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

fn mpu_bridge_init_check() {
    let dregion = (unsafe { read_volatile(MPU_TYPE) } >> 8) & 0xFF;
    if dregion != REQUIRED_DREGION {
        hprintln!(
            "gust-iso-contain-ctl FAIL: MPU_TYPE.DREGION={} (require {}); refusing to start",
            dregion,
            REQUIRED_DREGION
        );
        debug::exit(debug::EXIT_FAILURE);
    }
}

// In the control, ANY MemManage is a failure — no resume is ever armed.
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
        write_volatile(MPU_CTRL, 0);
        cortex_m::asm::dsb();
        cortex_m::asm::isb();
        hprintln!(
            "gust-iso-contain-ctl FAIL: unexpected MemManage pc={:#010x} CFSR={:#010x} MMFAR={:#010x} — the fixed tenant must run clean under the straddle placement",
            read_volatile(frame.add(6)),
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
        "gust-iso-contain-ctl FAIL: HardFault pc={:#010x} CFSR={:#010x} MMFAR={:#010x}",
        ef.pc(),
        unsafe { read_volatile(CFSR) },
        unsafe { read_volatile(MMFAR) }
    );
    debug::exit(debug::EXIT_FAILURE);
    loop {}
}

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
    static __synth_wasm_seg_0: u8;
    static __synth_wasm_seg_2: u8;
    static __iso_stale_data_start: u8;
    static __iso_stale_data_end: u8;
    static __iso_stale_data_lma: u8;
}

#[entry]
fn main() -> ! {
    mpu_bridge_init_check();
    unsafe {
        write_volatile(SHCSR, read_volatile(SHCSR) | (1 << 16));
        // Initialise .iso_stale_data (MPU still off).
        let start = addr_of!(__iso_stale_data_start) as *mut u8;
        let end = addr_of!(__iso_stale_data_end) as *const u8;
        let lma = addr_of!(__iso_stale_data_lma) as *const u8;
        let n = end as usize - start as usize;
        for i in 0..n {
            write_volatile(start.add(i), read_volatile(lma.add(i)));
        }
    }

    // Same placement pre-flight as the probe: identical arrangement or bust.
    let seg0 = addr_of!(__synth_wasm_seg_0) as u32;
    let seg2 = addr_of!(__synth_wasm_seg_2) as u32;
    if seg0 != STALE_DATA_VMA || seg2 != STALE_DATA_VMA + 0x18 {
        hprintln!(
            "gust-iso-contain-ctl FAIL: placement pre-flight (seg_0={:#010x} seg_2={:#010x}, want {:#010x}/{:#010x})",
            seg0,
            seg2,
            STALE_DATA_VMA,
            STALE_DATA_VMA + 0x18
        );
        debug::exit(debug::EXIT_FAILURE);
    }

    // Identical verified-core MPU map to gust_iso_contain_probe.
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
            "gust-iso-contain-ctl FAIL: MPU_CTRL={:#x} after verified switch (want {:#x})",
            ctrl,
            MPU_CTRL_ENABLE
        );
        debug::exit(debug::EXIT_FAILURE);
    }

    let r = unsafe { run_tl() };

    unsafe {
        write_volatile(MPU_CTRL, 0);
    }
    cortex_m::asm::dsb();
    cortex_m::asm::isb();
    let (len, captured) = unsafe { (read_volatile(addr_of!(LOG_LEN)), *addr_of!(LOG_BUF)) };
    let want: &[u8] = b"gust:os up\n";
    if r == 0xA && len == want.len() && &captured[..len] == want {
        hprintln!(
            "gust-iso-contain-ctl OK: fixed tenant ran CLEAN under the identical straddle placement + verified MPU map (run()=0xA, log==\"gust:os up\\n\") — containment in the probe is pinned to the miscompiled read"
        );
        debug::exit(debug::EXIT_SUCCESS);
    } else {
        hprintln!(
            "gust-iso-contain-ctl FAIL: run()={:#x} log_len={} log_bytes={:?} (want 0xA and \"gust:os up\\n\")",
            r,
            len,
            &captured[..len]
        );
        debug::exit(debug::EXIT_FAILURE);
    }
    loop {}
}
