//! gust-wdg-probe — the LOCAL qemu-semihosting probe of the DISSOLVED wdg-thin .o,
//! run BEFORE the Renode content-gate to catch table/linmem no-op bugs fast.
//!
//! Points the dissolved IWDG driver at a plain `[u32; 8]` RAM window (real mapped
//! SRAM on lm3s6965evb) and checks the exact key-sequence register effects (0x5555
//! unlock / PR+RLR config / 0xCCCC start / 0xAAAA refresh) plus the FSM via
//! semihosting — so a dissolved primitive that silently no-ops (e.g. a `.rodata`
//! linmem lookup that reads 0 under `--relocatable`) fails HERE, on `cargo run`,
//! not three CI minutes later in Renode. Also DEMONSTRATES the driver's distinctive
//! safety property, cannot-un-start: once Running, every attempt to unlock,
//! reconfigure, re-lock, or restart is rejected (WDG_FAULT, no register write) and
//! `wdg_is_running` stays 1 across a refresh — there is no software path out.
#![no_std]
#![no_main]
use core::ptr::{addr_of_mut, read_volatile, write_volatile};
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use panic_halt as _;

// The mmio seam the dissolved driver imports — here it reads/writes the RAM window.
#[no_mangle]
pub extern "C" fn mmio_read32(addr: u32) -> u32 {
    unsafe { read_volatile(addr as *const u32) }
}
#[no_mangle]
pub extern "C" fn mmio_write32(addr: u32, val: u32) {
    unsafe { write_volatile(addr as *mut u32, val) }
}

extern "C" {
    fn wdg_unlock(base: u32, state: u32) -> u32;
    fn wdg_configure(base: u32, state: u32, prescaler: u32, reload: u32) -> u32;
    fn wdg_lock(state: u32) -> u32;
    fn wdg_start(base: u32, state: u32) -> u32;
    fn wdg_refresh(base: u32, state: u32) -> u32;
    fn wdg_is_running(state: u32) -> u32;
}

// Fake IWDG register window in RAM: word i = byte offset i*4 (KR=0, PR=1, RLR=2, SR=3).
static mut REG: [u32; 8] = [0; 8];
const KEY_ENABLE: u32 = 0x5555;
const KEY_START: u32 = 0xCCCC;
const KEY_REFRESH: u32 = 0xAAAA;
const WDG_FAULT: u32 = 0xFFFF_FFFF;
const PRESCALER: u32 = 5; // 3-bit field, 5 & 0x7 = 5
const RELOAD: u32 = 0x123; // 12-bit field, 0x123 & 0xFFF = 0x123

#[entry]
fn main() -> ! {
    let base = addr_of_mut!(REG) as u32;
    let mut ok = true;

    // 0) write-protection: config attempted straight from Idle (state=0) MUST be
    //    rejected — no KR/PR/RLR write. A dissolve that silently no-ops set_config's
    //    phase check would let this through.
    let cfg_from_idle = unsafe { wdg_configure(base, 0, PRESCALER, RELOAD) };
    let kr0 = unsafe { read_volatile(addr_of_mut!(REG[0])) };
    let pr0 = unsafe { read_volatile(addr_of_mut!(REG[1])) };
    let rlr0 = unsafe { read_volatile(addr_of_mut!(REG[2])) };
    if cfg_from_idle != WDG_FAULT || kr0 != 0 || pr0 != 0 || rlr0 != 0 {
        hprintln!(
            "wdg-protect FAIL: cfg={:#x} KR={:#x} PR={:#x} RLR={:#x}",
            cfg_from_idle, kr0, pr0, rlr0
        );
        ok = false;
    } else {
        hprintln!("wdg-protect ok: config-from-Idle faulted, no register write");
    }

    // 1) unlock: driver must actually WRITE KR=0x5555 (a no-op leaves it 0).
    let s1 = unsafe { wdg_unlock(base, 0) };
    let kr1 = unsafe { read_volatile(addr_of_mut!(REG[0])) };
    if kr1 != KEY_ENABLE || s1 == WDG_FAULT {
        hprintln!("wdg-unlock FAIL: KR={:#x} want {:#x}, s1={:#x}", kr1, KEY_ENABLE, s1);
        ok = false;
    } else {
        hprintln!("wdg-unlock ok: KR={:#x}", kr1);
    }

    // 2) configure: staged prescaler/reload land in PR/RLR exactly (masked to their
    //    field widths) now that the registers are unlocked.
    let s2 = unsafe { wdg_configure(base, s1, PRESCALER, RELOAD) };
    let pr2 = unsafe { read_volatile(addr_of_mut!(REG[1])) };
    let rlr2 = unsafe { read_volatile(addr_of_mut!(REG[2])) };
    if s2 == WDG_FAULT || pr2 != PRESCALER || rlr2 != RELOAD {
        hprintln!(
            "wdg-config FAIL: s2={:#x} PR={:#x} want {:#x} RLR={:#x} want {:#x}",
            s2, pr2, PRESCALER, rlr2, RELOAD
        );
        ok = false;
    } else {
        hprintln!("wdg-config ok: PR={:#x} RLR={:#x}", pr2, rlr2);
    }

    // 3) lock: Unlocked -> Configured, no register write (KR/PR/RLR unchanged).
    let s3 = unsafe { wdg_lock(s2) };
    let kr3 = unsafe { read_volatile(addr_of_mut!(REG[0])) };
    if s3 == WDG_FAULT || kr3 != KEY_ENABLE || unsafe { wdg_is_running(s3) } != 0 {
        hprintln!("wdg-lock FAIL: s3={:#x} KR={:#x} running={}", s3, kr3, unsafe {
            wdg_is_running(s3)
        });
        ok = false;
    } else {
        hprintln!("wdg-lock ok: s3={:#x}, KR unchanged={:#x}, running=0", s3, kr3);
    }

    // 4) start: driver must WRITE KR=0xCCCC; wdg_is_running flips to 1. Irreversible.
    let s4 = unsafe { wdg_start(base, s3) };
    let kr4 = unsafe { read_volatile(addr_of_mut!(REG[0])) };
    let running4 = unsafe { wdg_is_running(s4) };
    if s4 == WDG_FAULT || kr4 != KEY_START || running4 != 1 {
        hprintln!("wdg-start FAIL: s4={:#x} KR={:#x} running={}", s4, kr4, running4);
        ok = false;
    } else {
        hprintln!("wdg-start ok: KR={:#x} running={}", kr4, running4);
    }

    // 5) cannot-un-start: EVERY transition off Running is rejected — no register
    //    write escapes, and running stays 1. This is the driver's distinctive
    //    safety property: there is no disable/stop export at all.
    let bad_unlock = unsafe { wdg_unlock(base, s4) };
    let bad_config = unsafe { wdg_configure(base, s4, 0, 0) };
    let bad_lock = unsafe { wdg_lock(s4) };
    let bad_restart = unsafe { wdg_start(base, s4) };
    let kr5 = unsafe { read_volatile(addr_of_mut!(REG[0])) };
    let pr5 = unsafe { read_volatile(addr_of_mut!(REG[1])) };
    let rlr5 = unsafe { read_volatile(addr_of_mut!(REG[2])) };
    let running5 = unsafe { wdg_is_running(s4) };
    if bad_unlock != WDG_FAULT
        || bad_config != WDG_FAULT
        || bad_lock != WDG_FAULT
        || bad_restart != WDG_FAULT
        || kr5 != KEY_START // unchanged since the last real write (start)
        || pr5 != PRESCALER
        || rlr5 != RELOAD
        || running5 != 1
    {
        hprintln!(
            "wdg-cannot-un-start FAIL: unlock={:#x} config={:#x} lock={:#x} restart={:#x} KR={:#x} PR={:#x} RLR={:#x} running={}",
            bad_unlock, bad_config, bad_lock, bad_restart, kr5, pr5, rlr5, running5
        );
        ok = false;
    } else {
        hprintln!(
            "wdg-cannot-un-start ok: unlock/config/lock/restart all faulted while Running, \
             no register touched, running stays 1"
        );
    }

    // 6) refresh: the ONLY accepted transition from Running keeps it Running. The
    //    Health Monitor's per-cycle "kick" — missing it lets the hardware reset fire.
    let s5 = unsafe { wdg_refresh(base, s4) };
    let kr6 = unsafe { read_volatile(addr_of_mut!(REG[0])) };
    let running6 = unsafe { wdg_is_running(s5) };
    // a second refresh, to show the running state is stable across repeated kicks.
    let s6 = unsafe { wdg_refresh(base, s5) };
    let running7 = unsafe { wdg_is_running(s6) };
    if s5 == WDG_FAULT || kr6 != KEY_REFRESH || running6 != 1 || s6 == WDG_FAULT || running7 != 1 {
        hprintln!(
            "wdg-refresh FAIL: s5={:#x} KR={:#x} running6={} s6={:#x} running7={}",
            s5, kr6, running6, s6, running7
        );
        ok = false;
    } else {
        hprintln!("wdg-refresh ok: KR={:#x} running stays 1 across repeated refresh", kr6);
    }

    if ok {
        hprintln!("wdg-probe ALL OK");
        debug::exit(debug::EXIT_SUCCESS);
    } else {
        debug::exit(debug::EXIT_FAILURE);
    }
    loop {}
}
