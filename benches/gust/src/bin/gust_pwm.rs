//! gust-pwm — the thin-seam advanced-timer PWM driver driven bare-metal on gust, with a
//! self-checking Renode content-gate (gust-OS driver breadth close-out).
//!
//! Links ONLY the dissolved pwm-thin driver (mmio bridge, 0 new TCB atoms — PWM is a
//! pure-output path so the driver imports only mmio_write32); the raw USART1 poke is
//! trusted plumbing to report results. Asserts (a) phase-gating — set_duty/start before
//! configure is rejected and touches no register, (b) the exact PWM-mode config the
//! driver writes on a RAM-mapped timer window (PSC/ARR period, CCMR1 PWM-mode-1+preload
//! 0x68, CCER output-enable, CR1 ARPE, EGR.UG latch), (c) the Kani-proven duty clamp — a
//! commanded duty above the period is clamped so CCR1 never exceeds ARR, and (d) the
//! Kani-proven total+latching failsafe: `pwm_failsafe` clears MOE (BDTR=0) from Running,
//! and from Safe every start/set_duty is rejected with no register write (MOE stays off)
//! — only an explicit configure re-arms. Deterministic, no dependence on Renode's timer
//! peripheral model.
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
    fn pwm_configure(base: u32, state: u32, psc: u32, period: u32) -> u32;
    fn pwm_set_duty(base: u32, state: u32, duty: u32, period: u32) -> u32;
    fn pwm_start(base: u32, state: u32) -> u32;
    fn pwm_failsafe(base: u32, state: u32) -> u32;
    fn pwm_duty(state: u32) -> u32;
    fn pwm_is_safe(state: u32) -> u32;
}

const TIM: u32 = 0x4001_2C00; // RAM-mapped advanced-timer (TIM1) window in the gate .repl
const CR1: u32 = 0x00;
const EGR: u32 = 0x14;
const CCMR1: u32 = 0x18;
const CCER: u32 = 0x20;
const PSC: u32 = 0x28;
const ARR: u32 = 0x2C;
const CCR1: u32 = 0x34;
const BDTR: u32 = 0x44;

const CCMR1_PWM1: u32 = (0b110 << 4) | (1 << 3); // 0x68
const CCER_CC1E: u32 = 1 << 0;
const CR1_ARPE: u32 = 1 << 7; // 0x80
const CR1_ARPE_CEN: u32 = (1 << 7) | (1 << 0); // 0x81
const EGR_UG: u32 = 1 << 0;
const BDTR_MOE: u32 = 1 << 15; // 0x8000

const ST_OFF: u32 = 0;
const ST_CONFIGURED: u32 = 1 << 30;
const ST_SAFE: u32 = 3 << 30;
const PWM_FAULT: u32 = 0xFFFF_FFFF;

const PSC_VAL: u32 = 7;
const PERIOD: u32 = 1000;
const DUTY: u32 = 250;
const DUTY_OVER: u32 = 2000;

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

#[inline]
fn rd(off: u32) -> u32 {
    unsafe { read_volatile((TIM + off) as *const u32) }
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

        tx(b"pwm-gate begin\n");

        // 0) phase-gating: set_duty/start straight from Off (state=0) MUST be rejected,
        //    no CCR1/BDTR write.
        let sd_off = pwm_set_duty(TIM, ST_OFF, DUTY, PERIOD);
        let st_off = pwm_start(TIM, ST_OFF);
        let ccr_off = rd(CCR1);
        let bdtr_off = rd(BDTR);
        tx(if sd_off == PWM_FAULT && st_off == PWM_FAULT && ccr_off == 0 && bdtr_off == 0 {
            b"pwm-protect-ok\n"
        } else {
            b"pwm-protect-bad\n"
        });

        // 1) configure: Off → Configured; PSC/ARR period, CCMR1 PWM-mode-1+preload,
        //    CCER output-enable, CR1 ARPE, EGR.UG latch.
        let s1 = pwm_configure(TIM, ST_OFF, PSC_VAL, PERIOD);
        tx(
            if s1 == ST_CONFIGURED
                && rd(PSC) == PSC_VAL
                && rd(ARR) == PERIOD
                && rd(CCMR1) == CCMR1_PWM1
                && rd(CCER) == CCER_CC1E
                && rd(CR1) == CR1_ARPE
                && rd(EGR) == EGR_UG
            {
                b"pwm-config-ok\n"
            } else {
                b"pwm-config-bad\n"
            },
        );

        // 2) duty clamp (Kani p1): a duty ABOVE the period is clamped to the period, so
        //    CCR1 can never exceed ARR (no unintended 100% output).
        let s2 = pwm_set_duty(TIM, s1, DUTY_OVER, PERIOD);
        let ccr2 = rd(CCR1);
        tx(if s2 != PWM_FAULT && ccr2 == PERIOD && ccr2 <= rd(ARR) && pwm_duty(s2) == PERIOD {
            b"pwm-clamp-ok\n"
        } else {
            b"pwm-clamp-bad\n"
        });

        // 3) start: Configured → Running; BDTR.MOE main-output enable, CR1 CEN; the
        //    clamped duty is preserved through arming.
        let s3 = pwm_start(TIM, s2);
        tx(
            if (s3 >> 30) == 2
                && rd(BDTR) == BDTR_MOE
                && rd(CR1) == CR1_ARPE_CEN
                && pwm_duty(s3) == PERIOD
                && pwm_is_safe(s3) == 0
            {
                b"pwm-start-ok\n"
            } else {
                b"pwm-start-bad\n"
            },
        );

        // 4) failsafe total-off (Kani p2): from Running, failsafe clears MOE (BDTR=0) and
        //    stops the counter (CR1=0) → Safe, duty 0. Always succeeds.
        let s4 = pwm_failsafe(TIM, s3);
        tx(
            if s4 == ST_SAFE
                && rd(BDTR) == 0
                && rd(CR1) == 0
                && pwm_is_safe(s4) == 1
                && pwm_duty(s4) == 0
            {
                b"pwm-failsafe-ok\n"
            } else {
                b"pwm-failsafe-bad\n"
            },
        );

        // 5) failsafe latches (Kani p3): from Safe, every start/set_duty is rejected with
        //    NO register write (MOE stays off, BDTR=0); only configure re-arms.
        let bad_start = pwm_start(TIM, s4);
        let bad_duty = pwm_set_duty(TIM, s4, DUTY, PERIOD);
        let bdtr5 = rd(BDTR);
        let safe5 = pwm_is_safe(s4);
        let s5 = pwm_configure(TIM, s4, PSC_VAL, PERIOD);
        tx(
            if bad_start == PWM_FAULT
                && bad_duty == PWM_FAULT
                && bdtr5 == 0
                && safe5 == 1
                && s5 == ST_CONFIGURED
            {
                b"pwm-latch-ok\n"
            } else {
                b"pwm-latch-bad\n"
            },
        );

        tx(b"pwm-gate done\n");
    }
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}
