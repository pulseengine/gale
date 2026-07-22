//! gust-wdg — the thin-seam IWDG (independent watchdog) driver driven bare-metal on
//! gust, with a self-checking Renode content-gate (gust-OS driver breadth close-out).
//!
//! Links ONLY the dissolved wdg-thin driver (mmio bridge, 0 new TCB atoms); the raw
//! USART1 poke is trusted plumbing to report results. Asserts (a) write-protection —
//! a config attempt before unlock is rejected and touches no register, (b) the exact
//! KR key-sequence writes (0x5555 unlock / 0xCCCC start / 0xAAAA refresh) on a
//! RAM-mapped IWDG window, (c) PR/RLR carry the staged prescaler/reload, and (d) the
//! Kani-proven cannot-un-start property: once Running, every attempt to unlock,
//! reconfigure, re-lock, or restart is rejected with no register write, and
//! `wdg_is_running` stays 1 across a refresh — deterministic, no dependence on
//! Renode's IWDG peripheral model.
#![no_std]
#![no_main]
use core::ptr::{read_volatile, write_volatile};
use cortex_m_rt::entry;
use cortex_m_semihosting::debug;
use panic_halt as _;

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

const IWDG: u32 = 0x4000_3000; // RAM-mapped IWDG window in the gate .repl
const IWDG_KR: u32 = 0x00;
const IWDG_PR: u32 = 0x04;
const IWDG_RLR: u32 = 0x08;
const WDG_FAULT: u32 = 0xFFFF_FFFF;

const KEY_ENABLE: u32 = 0x5555;
const KEY_START: u32 = 0xCCCC;
const KEY_REFRESH: u32 = 0xAAAA;
const PRESCALER: u32 = 5;
const RELOAD: u32 = 0x123;

const USART1: u32 = 0x4001_3800;
const USART_SR: u32 = 0x00;
const USART_DR: u32 = 0x04;
const USART_BRR: u32 = 0x08;
const USART_CR1: u32 = 0x0C;
const TXE: u32 = 1 << 7;

fn tx(s: &[u8]) {
    for &b in s {
        unsafe {
            while read_volatile((USART1 + USART_SR) as *const u32) & TXE == 0 {}
            write_volatile((USART1 + USART_DR) as *mut u32, (b as u32) & 0xFF);
        }
    }
}

#[entry]
fn main() -> ! {
    unsafe {
        // enable GPIOA(PA9 TX), AFIO, USART1; PA9 → AF push-pull; USART1 8MHz/115200.
        const RCC_APB2ENR: u32 = 0x4002_1018;
        let e = read_volatile(RCC_APB2ENR as *const u32);
        write_volatile(RCC_APB2ENR as *mut u32, e | (1 << 0) | (1 << 2) | (1 << 14));
        const GPIOA_CRH: u32 = 0x4001_0804;
        let c = read_volatile(GPIOA_CRH as *const u32);
        write_volatile(GPIOA_CRH as *mut u32, (c & !(0xF << 4)) | (0xB << 4));
        write_volatile((USART1 + USART_BRR) as *mut u32, 0x45);
        write_volatile((USART1 + USART_CR1) as *mut u32, (1 << 13) | (1 << 3));

        tx(b"wdg-gate begin\n");

        // 0) write-protection: config straight from Idle (state=0) MUST be rejected,
        //    no KR/PR/RLR write.
        let cfg_from_idle = wdg_configure(IWDG, 0, PRESCALER, RELOAD);
        let kr0 = read_volatile((IWDG + IWDG_KR) as *const u32);
        let pr0 = read_volatile((IWDG + IWDG_PR) as *const u32);
        let rlr0 = read_volatile((IWDG + IWDG_RLR) as *const u32);
        tx(if cfg_from_idle == WDG_FAULT && kr0 == 0 && pr0 == 0 && rlr0 == 0 {
            b"wdg-protect-ok\n"
        } else {
            b"wdg-protect-bad\n"
        });

        // 1) unlock: the DRIVER computes and writes KR=0x5555.
        let s1 = wdg_unlock(IWDG, 0);
        let kr1 = read_volatile((IWDG + IWDG_KR) as *const u32);
        tx(if s1 != WDG_FAULT && kr1 == KEY_ENABLE {
            b"wdg-unlock-ok\n"
        } else {
            b"wdg-unlock-bad\n"
        });

        // 2) configure + lock: staged prescaler/reload land in PR/RLR exactly.
        let s2 = wdg_configure(IWDG, s1, PRESCALER, RELOAD);
        let pr2 = read_volatile((IWDG + IWDG_PR) as *const u32);
        let rlr2 = read_volatile((IWDG + IWDG_RLR) as *const u32);
        let s3 = wdg_lock(s2);
        tx(if s2 != WDG_FAULT && pr2 == PRESCALER && rlr2 == RELOAD && s3 != WDG_FAULT {
            b"wdg-config-ok\n"
        } else {
            b"wdg-config-bad\n"
        });

        // 3) start: the DRIVER writes KR=0xCCCC; running flips to 1. Irreversible.
        let s4 = wdg_start(IWDG, s3);
        let kr4 = read_volatile((IWDG + IWDG_KR) as *const u32);
        let running4 = wdg_is_running(s4);
        tx(if s4 != WDG_FAULT && kr4 == KEY_START && running4 == 1 {
            b"wdg-start-ok\n"
        } else {
            b"wdg-start-bad\n"
        });

        // 4) cannot-un-start (Kani-proven p2): every transition off Running is
        //    rejected — no register write escapes, running stays 1.
        let bad_unlock = wdg_unlock(IWDG, s4);
        let bad_config = wdg_configure(IWDG, s4, 0, 0);
        let bad_lock = wdg_lock(s4);
        let bad_restart = wdg_start(IWDG, s4);
        let kr5 = read_volatile((IWDG + IWDG_KR) as *const u32);
        let pr5 = read_volatile((IWDG + IWDG_PR) as *const u32);
        let rlr5 = read_volatile((IWDG + IWDG_RLR) as *const u32);
        let running5 = wdg_is_running(s4);
        tx(
            if bad_unlock == WDG_FAULT
                && bad_config == WDG_FAULT
                && bad_lock == WDG_FAULT
                && bad_restart == WDG_FAULT
                && kr5 == KEY_START
                && pr5 == PRESCALER
                && rlr5 == RELOAD
                && running5 == 1
            {
                b"wdg-cannot-un-start-ok\n"
            } else {
                b"wdg-cannot-un-start-bad\n"
            },
        );

        // 5) refresh: the ONLY accepted transition from Running keeps it Running.
        let s5 = wdg_refresh(IWDG, s4);
        let kr6 = read_volatile((IWDG + IWDG_KR) as *const u32);
        let running6 = wdg_is_running(s5);
        let s6 = wdg_refresh(IWDG, s5);
        let running7 = wdg_is_running(s6);
        tx(
            if s5 != WDG_FAULT && kr6 == KEY_REFRESH && running6 == 1 && s6 != WDG_FAULT && running7 == 1 {
                b"wdg-refresh-ok\n"
            } else {
                b"wdg-refresh-bad\n"
            },
        );

        tx(b"wdg-gate done\n");
    }
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}
