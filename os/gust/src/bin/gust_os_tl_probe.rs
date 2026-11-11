//! gust-os-tl-probe — LOCAL qemu-semihosting liveness probe of the gust:os v0.4.0
//! STEP-2 node (drivers/os-node/os-tl-cm3.o): an app importing gust:os {time, log} —
//! log.line is the FIRST BUFFER-CARRYING capability (list<u8>) crossing the syscall
//! seam — wac-plugged with a time provider + a log provider, meld-fused + dissolved
//! to one bounded-SRAM object exporting `run` and importing only `read32`/`write32`.
//! Proves the buffer-carrying compose is functionally live end-to-end: app builds
//! `b"gust:os up\n"`, calls log.line (which the log-provider writes byte-by-byte to
//! UART_DR via write32), then runs OS-time logic and returns 0xA on success. We
//! provide both mmio TCB atoms, call `run` (via an r11=0 trampoline — the object is
//! synth --native-pointer-abi, so r11 is the pinned wasm linmem base, same
//! convention as gust_control.rs), and check BOTH the captured UART bytes and the
//! return code.
#![no_std]
#![no_main]
use core::ptr::addr_of_mut;
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use panic_halt as _;

const UART_DR: u32 = 0x4001_3804; // USART1 data register (via gust:hal/mmio)
const CAP: usize = 32;

static mut LOG_BUF: [u8; CAP] = [0; CAP];
static mut LOG_LEN: usize = 0;

// The whole TCB the gust:os {time, log} node needs: one mmio read atom (the timer
// CNT) and one mmio write atom (UART_DR, where log-provider shifts out log.line).
#[no_mangle]
pub extern "C" fn read32(_addr: u32) -> u32 { 1000 }

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

// The dissolved tl-node was compiled with synth --native-pointer-abi, which pins
// r11 as the wasm linmem base (0 — the shared-memory arena's absolute addresses are
// used directly). Every buffer-touching function in the object (log.line's byte
// loop reads `[r11, idx]`) relies on r11 == 0 at entry and never restores it itself
// (same convention as gust_control.rs's control_step_packed trampoline). Callers
// don't get r11 == 0 for free, so wrap the raw `run` export in a 4-instruction
// r11=0 trampoline, same pattern as gust_control.rs.
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
}

#[entry]
fn main() -> ! {
    let r = unsafe { run_tl() };
    let (len, captured) = unsafe { (*addr_of_mut!(LOG_LEN), *addr_of_mut!(LOG_BUF)) };
    let want: &[u8] = b"gust:os up\n";
    let log_ok = len == want.len() && &captured[..len] == want;
    if log_ok && r == 0xA {
        hprintln!("gust-os-tl-probe OK: log==\"gust:os up\\n\", run()=0xA");
        debug::exit(debug::EXIT_SUCCESS);
    } else {
        hprintln!(
            "gust-os-tl-probe FAIL: log_len={} log_bytes={:?} run()={:#x} (want log==\"gust:os up\\n\" and 0xA)",
            len, &captured[..len], r
        );
        debug::exit(debug::EXIT_FAILURE);
    }
    loop {}
}
