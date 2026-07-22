//! gust-adc — the thin-seam STM32F1 ADC driver driven bare-metal on gust, with a
//! self-checking Renode content-gate (gust-OS driver breadth close-out).
//!
//! Links ONLY the dissolved adc-thin driver (mmio bridge, 0 new TCB atoms); the raw
//! USART1 poke is trusted plumbing to report results. Asserts (a) channel bounds — an
//! out-of-range channel is rejected and touches no register, (b) the exact CR2 ADON
//! write on enable, (c) table-free SMPR2/SQR3/SQR1 config on a RAM-mapped ADC window,
//! (d) the CR2 ADON|SWSTART write on start, and (e) the Kani-proven read-after-EOC
//! exactly-once / single-shot property: reading the data register before EOC (while
//! Converting) is rejected with no stale sample, a completed read consumes the 12-bit
//! DR value and lands Ready (never Converting), so the ADC never free-runs — reading
//! twice is rejected and re-converting demands an explicit start. Deterministic, no
//! dependence on Renode's ADC peripheral model.
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
    fn adc_configure(base: u32, channel: u32, sample_code: u32);
    fn adc_enable(base: u32, state: u32, channel: u32) -> u32;
    fn adc_start(base: u32, state: u32) -> u32;
    fn adc_poll(base: u32, state: u32) -> u32;
    fn adc_read(base: u32, state: u32) -> u32;
    fn adc_sample(state: u32) -> u32;
    fn adc_is_complete(state: u32) -> u32;
    fn adc_disable(base: u32, state: u32) -> u32;
}

const ADC1: u32 = 0x4001_2400; // RAM-mapped ADC1 window in the gate .repl
const SR: u32 = 0x00;
const CR2: u32 = 0x08;
const SMPR2: u32 = 0x10;
const SQR1: u32 = 0x2C;
const SQR3: u32 = 0x34;
const DR: u32 = 0x4C;

const SR_EOC: u32 = 1 << 1;
const CR2_ADON: u32 = 1 << 0;
const CR2_SWSTART: u32 = 1 << 22;
const ADC_FAULT: u32 = 0xFFFF_FFFF;

const PHASE_SHIFT: u32 = 30;
const PH_OFF: u32 = 0;
const PH_READY: u32 = 1;
const PH_CONVERTING: u32 = 2;
const PH_COMPLETE: u32 = 3;

const CH: u32 = 3;
const SAMPLE_CODE: u32 = 4;
const MAX_CHANNEL: u32 = 17;

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
fn phase_of(state: u32) -> u32 {
    state >> PHASE_SHIFT
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

        tx(b"adc-gate begin\n");

        // 0) channel bounds: enable with a channel above MAX_CHANNEL is rejected, no
        //    CR2 write.
        let bad = adc_enable(ADC1, 0, MAX_CHANNEL + 1);
        let cr2_bad = read_volatile((ADC1 + CR2) as *const u32);
        tx(if bad == ADC_FAULT && cr2_bad == 0 {
            b"adc-chanbound-ok\n"
        } else {
            b"adc-chanbound-bad\n"
        });

        // 1) enable: Off → Ready, the DRIVER writes CR2=ADON.
        let s1 = adc_enable(ADC1, 0, CH);
        let cr2_1 = read_volatile((ADC1 + CR2) as *const u32);
        tx(if s1 != ADC_FAULT && phase_of(s1) == PH_READY && cr2_1 == CR2_ADON {
            b"adc-enable-ok\n"
        } else {
            b"adc-enable-bad\n"
        });

        // 2) configure: table-free SMPR2/SQR3/SQR1 land exactly.
        //    smpr_bits(3,4)=4<<9=0x800; SQR3 SQ1=3; SQR1 length(1)=0; CR2=ADON.
        adc_configure(ADC1, CH, SAMPLE_CODE);
        let smpr2 = read_volatile((ADC1 + SMPR2) as *const u32);
        let sqr3 = read_volatile((ADC1 + SQR3) as *const u32);
        let sqr1 = read_volatile((ADC1 + SQR1) as *const u32);
        let cr2_2 = read_volatile((ADC1 + CR2) as *const u32);
        tx(if smpr2 == (SAMPLE_CODE << 9) && sqr3 == CH && sqr1 == 0 && cr2_2 == CR2_ADON {
            b"adc-config-ok\n"
        } else {
            b"adc-config-bad\n"
        });

        // 3) start: Ready → Converting, the DRIVER writes CR2=ADON|SWSTART.
        let s3 = adc_start(ADC1, s1);
        let cr2_3 = read_volatile((ADC1 + CR2) as *const u32);
        tx(if s3 != ADC_FAULT && phase_of(s3) == PH_CONVERTING && cr2_3 == (CR2_ADON | CR2_SWSTART) {
            b"adc-start-ok\n"
        } else {
            b"adc-start-bad\n"
        });

        // 4) read-after-EOC (Kani-proven): reading DR WHILE Converting — before EOC —
        //    is rejected, no stale sample, still not complete.
        let stale = adc_read(ADC1, s3);
        let complete_mid = adc_is_complete(s3);
        tx(if stale == ADC_FAULT && complete_mid == 0 {
            b"adc-no-stale-ok\n"
        } else {
            b"adc-no-stale-bad\n"
        });

        // 5) poll: seed SR.EOC (hardware would set it), then Converting → Complete.
        write_volatile((ADC1 + SR) as *mut u32, SR_EOC);
        let s5 = adc_poll(ADC1, s3);
        let complete5 = adc_is_complete(s5);
        tx(if s5 != ADC_FAULT && phase_of(s5) == PH_COMPLETE && complete5 == 1 {
            b"adc-poll-ok\n"
        } else {
            b"adc-poll-bad\n"
        });

        // 6) read: from Complete, consume DR (12-bit masked) → Ready. DR=0x1ABC proves
        //    the mask (sample=0xABC).
        write_volatile((ADC1 + DR) as *mut u32, 0x1ABC);
        let s6 = adc_read(ADC1, s5);
        let sample = adc_sample(s6);
        tx(if s6 != ADC_FAULT && phase_of(s6) == PH_READY && sample == 0x0ABC {
            b"adc-read-ok\n"
        } else {
            b"adc-read-bad\n"
        });

        // 7) single-shot: the read landed Ready (not Converting); reading again is
        //    rejected and re-converting needs an explicit start.
        let twice = adc_read(ADC1, s6);
        let rearm = adc_start(ADC1, s6);
        tx(if twice == ADC_FAULT && rearm != ADC_FAULT && phase_of(rearm) == PH_CONVERTING {
            b"adc-single-shot-ok\n"
        } else {
            b"adc-single-shot-bad\n"
        });

        // 8) disable is total: from ANY state → Off, driver drives CR2=0.
        let s8 = adc_disable(ADC1, rearm);
        let cr2_8 = read_volatile((ADC1 + CR2) as *const u32);
        tx(if phase_of(s8) == PH_OFF && cr2_8 == 0 {
            b"adc-disable-ok\n"
        } else {
            b"adc-disable-bad\n"
        });

        tx(b"adc-gate done\n");
    }
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}
