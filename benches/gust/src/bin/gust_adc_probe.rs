//! gust-adc-probe — the LOCAL qemu-semihosting probe of the DISSOLVED adc-thin .o,
//! run BEFORE the Renode content-gate to catch table/linmem no-op bugs fast.
//!
//! Points the dissolved STM32F1 ADC driver at a plain `[u32; 24]` RAM window (real
//! mapped SRAM on lm3s6965evb) and checks the exact register effects — CR2 ADON on
//! enable, SMPRx/SQR3/SQR1 config (table-free bit arithmetic), CR2 ADON|SWSTART on
//! start, DR read on completion — plus the FSM via semihosting, so a dissolved
//! primitive that silently no-ops (e.g. a `.rodata` linmem lookup that reads 0 under
//! `--relocatable`) fails HERE, on `cargo run`, not three CI minutes later in Renode.
//! Also DEMONSTRATES the driver's distinctive safety property, read-after-EOC
//! exactly-once / single-shot: the data register may be read ONLY from Complete
//! (reading before EOC while Converting is rejected — no stale sample), a completed
//! read lands Ready (never Converting), so the ADC never free-runs — reading twice is
//! rejected and re-converting demands an explicit start.
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
    fn adc_configure(base: u32, channel: u32, sample_code: u32, cr2_extra: u32);
    fn adc_enable(base: u32, state: u32, channel: u32, cr2_extra: u32) -> u32;
    fn adc_start(base: u32, state: u32, cr2_extra: u32) -> u32;
    fn adc_poll(base: u32, state: u32) -> u32;
    fn adc_read(base: u32, state: u32) -> u32;
    fn adc_sample(state: u32) -> u32;
    fn adc_is_complete(state: u32) -> u32;
    fn adc_disable(base: u32, state: u32) -> u32;
}

// Fake STM32F1 ADC register window in RAM: word i = byte offset i*4. Register offsets
// (from the driver map): SR=0x00, CR2=0x08, SMPR1=0x0C, SMPR2=0x10, SQR1=0x2C,
// SQR3=0x34, DR=0x4C — top offset 0x4C = word 19, so 24 words covers it with margin.
static mut REG: [u32; 24] = [0; 24];
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

// Packed-state field layout (must match the driver's dissolve ABI).
const PHASE_SHIFT: u32 = 30;
const PH_OFF: u32 = 0;
const PH_READY: u32 = 1;
const PH_CONVERTING: u32 = 2;
const PH_COMPLETE: u32 = 3;

const CH: u32 = 3; // channel 3 → lives in SMPR2 (channels 0..9), field at (3%10)*3 = 9
const SAMPLE_CODE: u32 = 4; // 3-bit sample-time code
const MAX_CHANNEL: u32 = 17;

#[inline]
fn phase_of(state: u32) -> u32 {
    state >> PHASE_SHIFT
}
#[inline]
fn rw(off: u32) -> u32 {
    unsafe { read_volatile((addr_of_mut!(REG) as u32 + off) as *const u32) }
}
#[inline]
fn seed(off: u32, val: u32) {
    unsafe { write_volatile((addr_of_mut!(REG) as u32 + off) as *mut u32, val) }
}

#[entry]
fn main() -> ! {
    let base = addr_of_mut!(REG) as u32;
    let mut ok = true;

    // 0) channel bounds: enable with a channel above MAX_CHANNEL (18) MUST be rejected
    //    — ADC_FAULT, no CR2 write. A dissolve that silently no-ops the bound check
    //    would latch an out-of-range mux input.
    let bad = unsafe { adc_enable(base, 0, MAX_CHANNEL + 1, 0) };
    let cr2_after_bad = rw(CR2);
    if bad != ADC_FAULT || cr2_after_bad != 0 {
        hprintln!("adc-chanbound FAIL: enable(18)={:#x} CR2={:#x}", bad, cr2_after_bad);
        ok = false;
    } else {
        hprintln!("adc-chanbound ok: enable(18) faulted, no CR2 write");
    }

    // 1) enable: Off(state=0) → Ready, driver WRITES CR2=ADON (a no-op leaves it 0).
    let s1 = unsafe { adc_enable(base, 0, CH, 0) };
    let cr2_1 = rw(CR2);
    if s1 == ADC_FAULT || phase_of(s1) != PH_READY || cr2_1 != CR2_ADON {
        hprintln!("adc-enable FAIL: s1={:#x} phase={} CR2={:#x}", s1, phase_of(s1), cr2_1);
        ok = false;
    } else {
        hprintln!("adc-enable ok: Ready, CR2=ADON={:#x}", cr2_1);
    }

    // 2) configure: table-free bit arithmetic lands in SMPR2/SQR3/SQR1 exactly.
    //    smpr_bits(3,4) = 4 << ((3%10)*3) = 4 << 9 = 0x800 in SMPR2; SQR3 SQ1 = 3;
    //    SQR1 length(1) = 0; CR2 = ADON (single-shot, CONT=0).
    unsafe { adc_configure(base, CH, SAMPLE_CODE, 0) };
    let smpr2 = rw(SMPR2);
    let sqr3 = rw(SQR3);
    let sqr1 = rw(SQR1);
    let cr2_2 = rw(CR2);
    if smpr2 != (SAMPLE_CODE << 9) || sqr3 != CH || sqr1 != 0 || cr2_2 != CR2_ADON {
        hprintln!(
            "adc-config FAIL: SMPR2={:#x} want {:#x} SQR3={:#x} SQR1={:#x} CR2={:#x}",
            smpr2, SAMPLE_CODE << 9, sqr3, sqr1, cr2_2
        );
        ok = false;
    } else {
        hprintln!("adc-config ok: SMPR2={:#x} SQR3={:#x} SQR1=0 CR2=ADON", smpr2, sqr3);
    }

    // 3) start: Ready → Converting, driver WRITES CR2=ADON|SWSTART (keeps CONT=0).
    let s3 = unsafe { adc_start(base, s1, 0) };
    let cr2_3 = rw(CR2);
    if s3 == ADC_FAULT || phase_of(s3) != PH_CONVERTING || cr2_3 != (CR2_ADON | CR2_SWSTART) {
        hprintln!("adc-start FAIL: s3={:#x} phase={} CR2={:#x}", s3, phase_of(s3), cr2_3);
        ok = false;
    } else {
        hprintln!("adc-start ok: Converting, CR2=ADON|SWSTART={:#x}", cr2_3);
    }

    // 4) read-after-EOC (distinctive): reading DR WHILE Converting — before EOC — is
    //    rejected (ADC_FAULT), no phase change. This is the stale-sample bug the FSM
    //    forbids; a no-op that "reads anyway" would return a garbage sample here.
    let stale = unsafe { adc_read(base, s3) };
    let complete_mid = unsafe { adc_is_complete(s3) };
    if stale != ADC_FAULT || complete_mid != 0 {
        hprintln!("adc-no-stale FAIL: read-while-converting={:#x} is_complete={}", stale, complete_mid);
        ok = false;
    } else {
        hprintln!("adc-no-stale ok: read before EOC faulted, still not complete");
    }

    // 5) poll: seed SR.EOC (the hardware would set it), then Converting → Complete.
    //    adc_poll spins until SR.EOC and must NOT read DR (EOC stays set for read).
    seed(SR, SR_EOC);
    let s5 = unsafe { adc_poll(base, s3) };
    let complete5 = unsafe { adc_is_complete(s5) };
    if s5 == ADC_FAULT || phase_of(s5) != PH_COMPLETE || complete5 != 1 {
        hprintln!("adc-poll FAIL: s5={:#x} phase={} is_complete={}", s5, phase_of(s5), complete5);
        ok = false;
    } else {
        hprintln!("adc-poll ok: EOC observed → Complete, is_complete=1");
    }

    // 6) read: from Complete, driver reads DR and stores the 12-bit sample, → Ready.
    //    Seed DR with 0x1ABC — bit 12 set — to prove the 12-bit mask (sample=0xABC).
    seed(DR, 0x1ABC);
    let s6 = unsafe { adc_read(base, s5) };
    let sample = unsafe { adc_sample(s6) };
    if s6 == ADC_FAULT || phase_of(s6) != PH_READY || sample != 0x0ABC {
        hprintln!("adc-read FAIL: s6={:#x} phase={} sample={:#x} want 0xabc", s6, phase_of(s6), sample);
        ok = false;
    } else {
        hprintln!("adc-read ok: DR consumed → Ready, sample={:#x} (12-bit masked)", sample);
    }

    // 7) single-shot: a completed read landed Ready (NOT Converting), so the ADC never
    //    free-runs. Reading AGAIN from Ready is rejected (exactly-once), and the only
    //    way back to Converting is an explicit start — not an implicit re-arm.
    let twice = unsafe { adc_read(base, s6) };
    let rearm = unsafe { adc_start(base, s6, 0) };
    if twice != ADC_FAULT || rearm == ADC_FAULT || phase_of(rearm) != PH_CONVERTING {
        hprintln!("adc-single-shot FAIL: read-twice={:#x} restart={:#x} phase={}", twice, rearm, phase_of(rearm));
        ok = false;
    } else {
        hprintln!("adc-single-shot ok: read-twice faulted, re-convert needs explicit start");
    }

    // 8) disable is total: from ANY state → Off, driver drives CR2=0 (ADON off).
    let s8 = unsafe { adc_disable(base, rearm) };
    let cr2_8 = rw(CR2);
    if phase_of(s8) != PH_OFF || cr2_8 != 0 {
        hprintln!("adc-disable FAIL: s8={:#x} phase={} CR2={:#x}", s8, phase_of(s8), cr2_8);
        ok = false;
    } else {
        hprintln!("adc-disable ok: → Off, CR2=0");
    }

    if ok {
        hprintln!("adc-probe ALL OK");
        debug::exit(debug::EXIT_SUCCESS);
    } else {
        debug::exit(debug::EXIT_FAILURE);
    }
    loop {}
}
