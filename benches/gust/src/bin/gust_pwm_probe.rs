//! gust-pwm-probe — the LOCAL qemu-semihosting probe of the DISSOLVED pwm-thin .o,
//! run BEFORE the Renode content-gate to catch table/linmem no-op bugs fast.
//!
//! Points the dissolved advanced-timer PWM driver at a plain `[u32; 32]` RAM window
//! (real mapped SRAM on lm3s6965evb) and checks the exact register effects the DRIVER
//! computes — PSC/ARR period load, CCMR1 PWM-mode-1 + preload (0x68), CCER output
//! enable, CR1 ARPE, EGR.UG latch, CCR1 duty, BDTR.MOE main-output enable — plus the
//! FSM via semihosting, so a dissolved primitive that silently no-ops (e.g. a `.rodata`
//! linmem lookup that reads 0 under `--relocatable`) fails HERE, on `cargo run`, not
//! three CI minutes later in Renode. Also DEMONSTRATES the driver's two distinctive
//! safety properties: (1) the **duty clamp** — a commanded duty above the period is
//! clamped so CCR1 can never exceed ARR (no unintended 100% / full-throttle output);
//! (2) **failsafe is total and latching** — `pwm_failsafe` clears MOE from any state,
//! and from Safe every `start`/`set_duty` is rejected with NO register write (BDTR
//! stays 0) — only an explicit `configure` re-arms.
#![no_std]
#![no_main]
use core::ptr::{addr_of_mut, read_volatile, write_volatile};
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use panic_halt as _;

// The mmio seam the dissolved driver imports — here it writes the RAM window. pwm-thin
// is a pure-output path (it never reads a register), so only mmio_write32 is undefined
// in the object; mmio_read32 is provided for completeness of the bridge shape.
#[no_mangle]
pub extern "C" fn mmio_read32(addr: u32) -> u32 {
    unsafe { read_volatile(addr as *const u32) }
}
#[no_mangle]
pub extern "C" fn mmio_write32(addr: u32, val: u32) {
    unsafe { write_volatile(addr as *mut u32, val) }
}

extern "C" {
    fn pwm_configure(base: u32, state: u32, psc: u32, period: u32) -> u32;
    fn pwm_set_duty(base: u32, state: u32, duty: u32, period: u32) -> u32;
    fn pwm_start(base: u32, state: u32) -> u32;
    fn pwm_failsafe(base: u32, state: u32) -> u32;
    fn pwm_duty(state: u32) -> u32;
    fn pwm_is_safe(state: u32) -> u32;
}

// Fake advanced-timer register window in RAM: byte offsets from base (word-aligned).
static mut REG: [u32; 32] = [0; 32];
const CR1: u32 = 0x00;
const EGR: u32 = 0x14;
const CCMR1: u32 = 0x18;
const CCER: u32 = 0x20;
const PSC: u32 = 0x28;
const ARR: u32 = 0x2C;
const CCR1: u32 = 0x34;
const BDTR: u32 = 0x44;

// Exact bit values the driver writes.
const CCMR1_PWM1: u32 = (0b110 << 4) | (1 << 3); // OC1M PWM mode 1 + OC1PE preload = 0x68
const CCER_CC1E: u32 = 1 << 0;
const CR1_ARPE: u32 = 1 << 7; // 0x80
const CR1_ARPE_CEN: u32 = (1 << 7) | (1 << 0); // 0x81 after start
const EGR_UG: u32 = 1 << 0;
const BDTR_MOE: u32 = 1 << 15; // 0x8000

// Packed FSM state: phase in bits[31:30], duty in bits[15:0].
const ST_OFF: u32 = 0;
const ST_CONFIGURED: u32 = 1 << 30;
const ST_SAFE: u32 = 3 << 30;
const PWM_FAULT: u32 = 0xFFFF_FFFF;

const PSC_VAL: u32 = 7;
const PERIOD: u32 = 1000; // 0x3E8
const DUTY: u32 = 250; // < period
const DUTY_OVER: u32 = 2000; // > period → must clamp to PERIOD

#[inline]
fn rd(base: u32, off: u32) -> u32 {
    unsafe { read_volatile((base + off) as *const u32) }
}

#[entry]
fn main() -> ! {
    let base = addr_of_mut!(REG) as u32;
    let mut ok = true;

    // 0) phase-gating / write-protection: set_duty and start attempted straight from Off
    //    (state=0) MUST be rejected — no CCR1/BDTR write. A dissolve that no-ops the
    //    phase check would let a duty onto an unconfigured output.
    let sd_off = unsafe { pwm_set_duty(base, ST_OFF, DUTY, PERIOD) };
    let st_off = unsafe { pwm_start(base, ST_OFF) };
    let ccr_off = rd(base, CCR1);
    let bdtr_off = rd(base, BDTR);
    if sd_off != PWM_FAULT || st_off != PWM_FAULT || ccr_off != 0 || bdtr_off != 0 {
        hprintln!(
            "pwm-protect FAIL: set_duty={:#x} start={:#x} CCR1={:#x} BDTR={:#x}",
            sd_off, st_off, ccr_off, bdtr_off
        );
        ok = false;
    } else {
        hprintln!("pwm-protect ok: set_duty/start-from-Off faulted, no CCR1/BDTR write");
    }

    // 1) configure: Off → Configured; PSC/ARR carry the period, CCMR1 is PWM-mode-1 +
    //    preload (0x68), CCER enables the output, CR1 sets ARPE, EGR.UG latches.
    let s1 = unsafe { pwm_configure(base, ST_OFF, PSC_VAL, PERIOD) };
    let psc1 = rd(base, PSC);
    let arr1 = rd(base, ARR);
    let ccmr1 = rd(base, CCMR1);
    let ccer1 = rd(base, CCER);
    let cr1_1 = rd(base, CR1);
    let egr1 = rd(base, EGR);
    if s1 != ST_CONFIGURED
        || psc1 != PSC_VAL
        || arr1 != PERIOD
        || ccmr1 != CCMR1_PWM1
        || ccer1 != CCER_CC1E
        || cr1_1 != CR1_ARPE
        || egr1 != EGR_UG
    {
        hprintln!(
            "pwm-config FAIL: s1={:#x} PSC={:#x} ARR={:#x} CCMR1={:#x} CCER={:#x} CR1={:#x} EGR={:#x}",
            s1, psc1, arr1, ccmr1, ccer1, cr1_1, egr1
        );
        ok = false;
    } else {
        hprintln!(
            "pwm-config ok: Configured, PSC={:#x} ARR={:#x} CCMR1={:#x}(PWM1+preload) CCER={:#x} CR1={:#x} EGR.UG={:#x}",
            psc1, arr1, ccmr1, ccer1, cr1_1, egr1
        );
    }

    // 2) set_duty within the period: CCR1 lands the exact commanded duty; still Configured.
    let s2 = unsafe { pwm_set_duty(base, s1, DUTY, PERIOD) };
    let ccr2 = rd(base, CCR1);
    let d2 = unsafe { pwm_duty(s2) };
    if s2 == PWM_FAULT || (s2 >> 30) != 1 || ccr2 != DUTY || d2 != DUTY {
        hprintln!("pwm-duty FAIL: s2={:#x} CCR1={:#x} want {:#x} pwm_duty={:#x}", s2, ccr2, DUTY, d2);
        ok = false;
    } else {
        hprintln!("pwm-duty ok: CCR1={:#x} == commanded {:#x}, still Configured", ccr2, DUTY);
    }

    // 3) DISTINCTIVE P1 — duty clamp: a duty ABOVE the period is clamped to the period,
    //    so CCR1 can NEVER exceed ARR (no unintended 100% / full-throttle output).
    let s3 = unsafe { pwm_set_duty(base, s1, DUTY_OVER, PERIOD) };
    let ccr3 = rd(base, CCR1);
    let d3 = unsafe { pwm_duty(s3) };
    if s3 == PWM_FAULT || ccr3 != PERIOD || d3 != PERIOD || ccr3 > arr1 {
        hprintln!(
            "pwm-clamp FAIL: commanded {:#x} CCR1={:#x} want {:#x}(=ARR) pwm_duty={:#x}",
            DUTY_OVER, ccr3, PERIOD, d3
        );
        ok = false;
    } else {
        hprintln!(
            "pwm-clamp ok: duty {:#x} > period clamped → CCR1={:#x} == ARR, never exceeds period",
            DUTY_OVER, ccr3
        );
    }

    // 4) start: Configured → Running; BDTR.MOE enables the main output, CR1 sets CEN
    //    (0x81); the staged (clamped) duty is preserved through arming, not_safe.
    let s4 = unsafe { pwm_start(base, s3) };
    let bdtr4 = rd(base, BDTR);
    let cr1_4 = rd(base, CR1);
    let d4 = unsafe { pwm_duty(s4) };
    let safe4 = unsafe { pwm_is_safe(s4) };
    if (s4 >> 30) != 2 || bdtr4 != BDTR_MOE || cr1_4 != CR1_ARPE_CEN || d4 != PERIOD || safe4 != 0 {
        hprintln!(
            "pwm-start FAIL: s4={:#x} BDTR={:#x} CR1={:#x} pwm_duty={:#x} is_safe={}",
            s4, bdtr4, cr1_4, d4, safe4
        );
        ok = false;
    } else {
        hprintln!(
            "pwm-start ok: Running, BDTR.MOE={:#x} CR1={:#x}(CEN) duty preserved={:#x} is_safe=0",
            bdtr4, cr1_4, d4
        );
    }

    // 5) set_duty while Running is still accepted (armed) — CCR1 updates live.
    let s5 = unsafe { pwm_set_duty(base, s4, DUTY, PERIOD) };
    let ccr5 = rd(base, CCR1);
    if s5 == PWM_FAULT || (s5 >> 30) != 2 || ccr5 != DUTY {
        hprintln!("pwm-run-duty FAIL: s5={:#x} CCR1={:#x} want {:#x}", s5, ccr5, DUTY);
        ok = false;
    } else {
        hprintln!("pwm-run-duty ok: Running set_duty → CCR1={:#x}, stays Running", ccr5);
    }

    // 6) DISTINCTIVE P2a — failsafe total-off: from Running, `failsafe` clears MOE (BDTR
    //    = 0) and stops the counter (CR1 = 0) → Safe, duty 0. Always succeeds.
    let s6 = unsafe { pwm_failsafe(base, s5) };
    let bdtr6 = rd(base, BDTR);
    let cr1_6 = rd(base, CR1);
    let safe6 = unsafe { pwm_is_safe(s6) };
    let d6 = unsafe { pwm_duty(s6) };
    if s6 != ST_SAFE || bdtr6 != 0 || cr1_6 != 0 || safe6 != 1 || d6 != 0 {
        hprintln!(
            "pwm-failsafe FAIL: s6={:#x} BDTR={:#x} CR1={:#x} is_safe={} pwm_duty={:#x}",
            s6, bdtr6, cr1_6, safe6, d6
        );
        ok = false;
    } else {
        hprintln!("pwm-failsafe ok: Safe, MOE cleared (BDTR=0) CR1=0 is_safe=1 duty=0");
    }

    // 7) DISTINCTIVE P2b — failsafe latches: from Safe, EVERY start/set_duty is rejected
    //    with NO register write (BDTR stays 0, no MOE re-enable) and is_safe stays 1 —
    //    only an explicit `configure` re-arms. A failsafe you can casually clear is
    //    worthless.
    let bad_start = unsafe { pwm_start(base, s6) };
    let bad_duty = unsafe { pwm_set_duty(base, s6, DUTY, PERIOD) };
    let bdtr7 = rd(base, BDTR);
    let safe7 = unsafe { pwm_is_safe(s6) };
    // only configure leaves Safe.
    let s7 = unsafe { pwm_configure(base, s6, PSC_VAL, PERIOD) };
    if bad_start != PWM_FAULT
        || bad_duty != PWM_FAULT
        || bdtr7 != 0 // no MOE re-enable escaped
        || safe7 != 1
        || s7 != ST_CONFIGURED
    {
        hprintln!(
            "pwm-latch FAIL: bad_start={:#x} bad_duty={:#x} BDTR={:#x} is_safe={} reconfigure={:#x}",
            bad_start, bad_duty, bdtr7, safe7, s7
        );
        ok = false;
    } else {
        hprintln!(
            "pwm-latch ok: from Safe start/set_duty faulted, MOE stayed off (BDTR=0), \
             only configure re-armed → Configured"
        );
    }

    if ok {
        hprintln!("pwm-probe ALL OK");
        debug::exit(debug::EXIT_SUCCESS);
    } else {
        debug::exit(debug::EXIT_FAILURE);
    }
    loop {}
}
