//! gust-dac — the thin-seam software-triggered DAC driver driven bare-metal on gust,
//! with a self-checking Renode content-gate (gust-OS driver breadth close-out).
//!
//! Links ONLY the dissolved dac-thin driver (mmio bridge, 0 new TCB atoms); the raw
//! USART1 poke is trusted plumbing to report results. Asserts (a) phase gating — a
//! `load`/`trigger` before `enable` is rejected and writes no DHR/SWTRIGR, (b) the
//! CR enable word the driver writes on `enable`, (c) DHR carries the staged 12-bit
//! code on `load` while the pin (DOR) does NOT move, (d) the Kani-proven glitch-free
//! property: after publishing CODE_A, staging CODE_B rewrites DHR but DOR still holds
//! CODE_A until the next trigger — no half-updated code is ever driven, (e) the
//! trigger publishes atomically (SWTRIGR write; DOR := staged code), and (f) range
//! clamp. Deterministic on a RAM-mapped DAC window; a plain RAM window has no DAC
//! peripheral to latch DHR->DOR on the software trigger, so the gate emulates that one
//! hardware step (DOR := DHR right after the driver's SWTRIGR write), exactly as
//! gust_spi's gate pre-seeds SR/DR. Everything else is the dissolved driver.
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
    fn dac_enable(base: u32, state: u32, channel: u32) -> u32;
    fn dac_load(base: u32, state: u32, v: u32) -> u32;
    fn dac_trigger(base: u32, state: u32) -> u32;
    fn dac_output(base: u32, state: u32) -> u32;
    fn dac_value(state: u32) -> u32;
    fn dac_is_output(state: u32) -> u32;
    fn dac_disable(base: u32, state: u32) -> u32;
}

const DAC: u32 = 0x4000_7400; // RAM-mapped DAC window in the gate .repl
const DAC_CR: u32 = 0x00;
const DAC_SWTRIGR: u32 = 0x04;
const DAC_DHR12R1: u32 = 0x08;
const DAC_DOR1: u32 = 0x2C;
const DAC_FAULT: u32 = 0xFFFF_FFFF;

const CR_EN_SW: u32 = 0x3D; // EN|TEN|TSEL(sw) for channel 1
const SWTRIG1: u32 = 1;
const CODE_A: u32 = 0xABC;
const CODE_B: u32 = 0x555;
const OVERRANGE: u32 = 0x1_2FFF;

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
    unsafe { read_volatile((DAC + off) as *const u32) }
}
// emulate the DAC's hardware DHR->DOR latch that the software trigger fires.
#[inline]
fn latch() {
    unsafe { write_volatile((DAC + DAC_DOR1) as *mut u32, rd(DAC_DHR12R1)) };
}

#[entry]
fn main() -> ! {
    unsafe {
        const RCC_APB2ENR: u32 = 0x4002_1018;
        let e = read_volatile(RCC_APB2ENR as *const u32);
        write_volatile(RCC_APB2ENR as *mut u32, e | (1 << 0) | (1 << 2) | (1 << 14));
        const GPIOA_CRH: u32 = 0x4001_0804;
        let c = read_volatile(GPIOA_CRH as *const u32);
        write_volatile(GPIOA_CRH as *mut u32, (c & !(0xF << 4)) | (0xB << 4));
        write_volatile((USART1 + USART_BRR) as *mut u32, 0x45);
        write_volatile((USART1 + USART_CR1) as *mut u32, (1 << 13) | (1 << 3));

        tx(b"dac-gate begin\n");

        // 0) phase gating: load/trigger before enable are rejected, no register write.
        let load_off = dac_load(DAC, 0, CODE_A);
        let dhr_off = rd(DAC_DHR12R1);
        let trig_off = dac_trigger(DAC, 0);
        let swtrig_off = rd(DAC_SWTRIGR);
        tx(if load_off == DAC_FAULT && dhr_off == 0 && trig_off == DAC_FAULT && swtrig_off == 0 {
            b"dac-phase-gate-ok\n"
        } else {
            b"dac-phase-gate-bad\n"
        });

        // 1) enable ch1: the DRIVER writes CR = 0x3D; phase Ready (not Output).
        let s1 = dac_enable(DAC, 0, 0);
        let cr1 = rd(DAC_CR);
        tx(if s1 != DAC_FAULT && cr1 == CR_EN_SW && dac_is_output(s1) == 0 {
            b"dac-enable-ok\n"
        } else {
            b"dac-enable-bad\n"
        });

        // 2) load CODE_A: DHR carries the staged code; DOR (pin) does NOT move.
        let s2 = dac_load(DAC, s1, CODE_A);
        let dhr2 = rd(DAC_DHR12R1);
        let dor2 = rd(DAC_DOR1);
        tx(
            if s2 != DAC_FAULT && dhr2 == CODE_A && dor2 == 0 && dac_is_output(s2) == 0
                && dac_value(s2) == CODE_A
            {
                b"dac-load-ok\n"
            } else {
                b"dac-load-bad\n"
            },
        );

        // 3) trigger publishes CODE_A: SWTRIGR write, latch DHR->DOR, pin == code.
        let s3 = dac_trigger(DAC, s2);
        let swtrig3 = rd(DAC_SWTRIGR);
        latch();
        let out3 = dac_output(DAC, s3);
        tx(
            if s3 != DAC_FAULT && swtrig3 == SWTRIG1 && out3 == CODE_A && dac_is_output(s3) == 1 {
                b"dac-trigger-ok\n"
            } else {
                b"dac-trigger-bad\n"
            },
        );

        // 4) GLITCH-FREE (Kani p3): stage CODE_B while CODE_A is on the pin. DHR
        //    updates to CODE_B but DOR STILL reads CODE_A — pin un-glitched.
        let s4 = dac_load(DAC, s3, CODE_B);
        let dhr4 = rd(DAC_DHR12R1);
        let dor4 = dac_output(DAC, s4);
        tx(
            if s4 != DAC_FAULT && dhr4 == CODE_B && dor4 == CODE_A && dac_is_output(s4) == 0
                && dac_value(s4) == CODE_B
            {
                b"dac-glitch-free-ok\n"
            } else {
                b"dac-glitch-free-bad\n"
            },
        );

        // 5) second trigger moves the pin atomically to CODE_B.
        let s5 = dac_trigger(DAC, s4);
        let swtrig5 = rd(DAC_SWTRIGR);
        latch();
        let out5 = dac_output(DAC, s5);
        tx(
            if s5 != DAC_FAULT && swtrig5 == SWTRIG1 && out5 == CODE_B && dac_is_output(s5) == 1 {
                b"dac-publish2-ok\n"
            } else {
                b"dac-publish2-bad\n"
            },
        );

        // 6) range clamp (Kani p1): an overrange command is masked to 12 bits in DHR.
        let s6 = dac_load(DAC, s5, OVERRANGE);
        let dhr6 = rd(DAC_DHR12R1);
        let want = OVERRANGE & 0x0FFF;
        tx(if s6 != DAC_FAULT && dhr6 == want && dac_value(s6) == want {
            b"dac-clamp-ok\n"
        } else {
            b"dac-clamp-bad\n"
        });

        // 7) disable is total: CR driven to 0, back to Off.
        let s7 = dac_disable(DAC, s6);
        let cr7 = rd(DAC_CR);
        tx(if cr7 == 0 && dac_is_output(s7) == 0 && dac_value(s7) == 0 {
            b"dac-disable-ok\n"
        } else {
            b"dac-disable-bad\n"
        });

        tx(b"dac-gate done\n");
    }
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}
