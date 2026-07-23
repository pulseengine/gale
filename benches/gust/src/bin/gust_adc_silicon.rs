//! gust-adc-silicon — REAL-HARDWARE anchor for the dissolved adc-thin driver.
//!
//! Unlike gust_adc_probe (qemu, a `[u32;24]` RAM window faking the ADC) and gust_adc
//! (Renode content-gate), this firmware points the DISSOLVED adc-thin-cm3.o at the
//! REAL STM32F100 ADC1 (0x4001_2400) and reads the on-chip **Vrefint** (channel 17,
//! the 1.2 V internal reference) — a self-contained silicon check needing no external
//! wiring. Flash + capture:  benches/gust/silicon/run-adc.sh
//!
//! Expected: Vrefint is factory-nominal ≈ 1.20 V (spec 1.16..1.24 V). The raw code
//! depends on the board's VDDA reference: on the STM32VLDISCOVERY VDDA ≈ **3.0 V**
//! (not 3.3 V) → raw ≈ 1.20/3.0*4095 ≈ **1638**. Rather than assume the rail, this
//! anchor checks the raw is in the plausible Vrefint band and *reports the implied
//! VDDA* back from the known 1.20 V (the classic "Vrefint measures VDDA" use): a raw
//! near 0 or full-scale = the internal channel was not actually converted.
//!
//! Reading an F1 internal channel needs `CR2.TSVREFE` (bit 23) set *during* the
//! conversion — which the driver's absolute CR2 writes used to drop (gale#216). Fixed
//! by threading `cr2_extra = TSVREFE` through adc_enable/adc_start so the dissolved
//! driver keeps TSVREFE on every managed CR2 write.
//!
//! Silicon-vs-model note (RESOLVED, gale#216): the driver used to write CR2=ADON in
//! BOTH adc_enable (wake) and adc_configure (config). On the RAM-window probe a
//! repeated ADON write was inert, but on real F1 a `1`-written-while-ADON=1 can kick a
//! spurious conversion. Fixed by dropping `adc_configure`'s CR2 write entirely —
//! ADON stays set from `adc_enable` and `adc_configure` now only touches
//! SMPR/SQR3/SQR1, so the F1 read is a strict single-shot: exactly one SWSTART in
//! `adc_start`.
//!
//! F1 ADC bring-up the FSM does NOT model (done here in firmware): enable the ADC1
//! clock (RCC APB2ENR), wake + tSTAB, and RSTCAL/CAL self-calibration — all one-time,
//! before the dissolved conversion lifecycle (enable→configure→start→poll→read).
#![no_std]
#![no_main]

use core::ptr::{read_volatile, write_volatile};
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use panic_halt as _;

// The mmio seam the dissolved driver imports — here it drives the REAL peripheral.
#[no_mangle]
pub extern "C" fn mmio_read32(addr: u32) -> u32 {
    unsafe { read_volatile(addr as *const u32) }
}
#[no_mangle]
pub extern "C" fn mmio_write32(addr: u32, val: u32) {
    unsafe { write_volatile(addr as *mut u32, val) }
}

// The dissolved adc-thin driver (state-threaded FSM; cr2_extra carries TSVREFE).
extern "C" {
    fn adc_enable(base: u32, state: u32, channel: u32, cr2_extra: u32) -> u32;
    fn adc_configure(base: u32, channel: u32, sample_code: u32);
    fn adc_start(base: u32, state: u32, cr2_extra: u32) -> u32;
    fn adc_poll(base: u32, state: u32) -> u32;
    fn adc_read(base: u32, state: u32) -> u32;
    fn adc_sample(state: u32) -> u32;
}

// ADC_BASE (0x4001_2400) + VREFINT_CH (17) are GENERATED from the AADL model
// (benches/gust/targets/) — see gust-target-gen. F100 only (thumbv7m).
#[cfg(not(feature = "target-f100"))]
compile_error!("gust_adc_silicon: build with --no-default-features --features target-f100 (Vrefint is an F1/F100 internal channel)");
#[cfg(feature = "target-f100")]
#[path = "../../targets/generated/gust_target_stm32f100.rs"]
#[allow(dead_code)]
mod target;
use target::{ADC_BASE, BOARD, VREFINT_CH};

// STM32F1 ADC register offsets + bits (device knowledge as data, mirrors the driver).
const CR2: u32 = 0x08;
const SR: u32 = 0x00;
const SR_EOC: u32 = 1 << 1;
const CR2_ADON: u32 = 1 << 0;
const CR2_CAL: u32 = 1 << 2;
const CR2_RSTCAL: u32 = 1 << 3;
const CR2_TSVREFE: u32 = 1 << 23; // enable the temp-sensor / Vrefint internal channels

// RCC — enable the ADC1 peripheral clock. At reset the F100 runs on HSI 8 MHz, so
// PCLK2 = 8 MHz and the default ADCPRE (/2) gives ADCCLK = 4 MHz (≤ 14 MHz) — no
// prescaler change needed.
const RCC_APB2ENR: u32 = 0x4002_1018;
const RCC_APB2ENR_ADC1EN: u32 = 1 << 9;

const SAMPLE_CODE: u32 = 7; // 239.5-cycle sample time — Vrefint needs a long Ts (≥17.1 µs)
const ADC_FAULT: u32 = 0xFFFF_FFFF;

// Vrefint (1.20 V nominal) raw band across a plausible VDDA of ~2.8..3.3 V: 1.20 V
// reads 1489 (@3.3) .. 1755 (@2.8). VLDISCOVERY runs ~3.0 V → ~1638. A raw outside
// this whole band means the internal channel was not converted (float/rail), not a
// mere rail difference.
const VREF_LO: u32 = 1450;
const VREF_HI: u32 = 1780;
const VREFINT_MV: u32 = 1200; // factory-nominal Vrefint, for the implied-VDDA report

#[inline]
fn rd(addr: u32) -> u32 {
    unsafe { read_volatile(addr as *const u32) }
}
#[inline]
fn wr(addr: u32, val: u32) {
    unsafe { write_volatile(addr as *mut u32, val) }
}
#[inline(always)]
fn delay(n: u32) {
    for _ in 0..n {
        cortex_m::asm::nop();
    }
}

#[entry]
fn main() -> ! {
    hprintln!(
        "gust-adc-silicon: reading Vrefint (ch{}) on real {} ADC1 @0x{:08x} via the \
         dissolved adc-thin driver (TSVREFE carried through cr2_extra)...",
        VREFINT_CH, BOARD, ADC_BASE
    );

    // --- F1 ADC bring-up the FSM does not model (one-time, firmware-side) ---
    // 1) ADC1 peripheral clock.
    wr(RCC_APB2ENR, rd(RCC_APB2ENR) | RCC_APB2ENR_ADC1EN);

    // 2) Wake the ADC + select Vrefint through the dissolved driver: Off -> Ready,
    //    CR2 = ADON | TSVREFE (first ADON write = power-up, no conversion).
    let s1 = unsafe { adc_enable(ADC_BASE, 0, VREFINT_CH, CR2_TSVREFE) };
    if s1 == ADC_FAULT {
        hprintln!("gust-adc-silicon FAIL: adc_enable(ch{}) faulted", VREFINT_CH);
        debug::exit(debug::EXIT_FAILURE);
        loop {}
    }
    delay(2000); // tSTAB (~1 µs @ ADCCLK) with wide margin before calibration

    // 3) Self-calibrate (RSTCAL then CAL), preserving ADON|TSVREFE. The driver does not
    //    model calibration; it is standard F1 one-time bring-up in the Ready phase.
    wr(ADC_BASE + CR2, CR2_ADON | CR2_TSVREFE | CR2_RSTCAL);
    while rd(ADC_BASE + CR2) & CR2_RSTCAL != 0 {}
    wr(ADC_BASE + CR2, CR2_ADON | CR2_TSVREFE | CR2_CAL);
    while rd(ADC_BASE + CR2) & CR2_CAL != 0 {}

    // --- dissolved conversion lifecycle: configure -> start -> poll -> read ---
    // configure: SMPR1 ch17 = 239.5 cyc, SQR3 = 17, SQR1 len = 1. Does NOT touch CR2
    // (gale#216) — CR2 stays ADON|TSVREFE from adc_enable above.
    unsafe { adc_configure(ADC_BASE, VREFINT_CH, SAMPLE_CODE) };
    // start: CR2 = ADON | SWSTART | TSVREFE -> conversion of ch17.
    let s2 = unsafe { adc_start(ADC_BASE, s1, CR2_TSVREFE) };
    if s2 == ADC_FAULT {
        hprintln!("gust-adc-silicon FAIL: adc_start faulted (s1=0x{:08x})", s1);
        debug::exit(debug::EXIT_FAILURE);
        loop {}
    }
    // poll to EOC, then consume the sample (read-after-EOC exactly-once).
    let s3 = unsafe { adc_poll(ADC_BASE, s2) };
    let s4 = unsafe { adc_read(ADC_BASE, s3) };
    let sample = unsafe { adc_sample(s4) };
    let eoc = rd(ADC_BASE + SR) & SR_EOC; // should be cleared by the DR read

    if s3 == ADC_FAULT || s4 == ADC_FAULT {
        hprintln!("gust-adc-silicon FAIL: poll/read faulted (s3=0x{:08x} s4=0x{:08x})", s3, s4);
        debug::exit(debug::EXIT_FAILURE);
        loop {}
    }

    if sample >= VREF_LO && sample <= VREF_HI {
        // Vrefint is known ≈ 1.20 V, so the sample implies VDDA = 1.20 V * 4095 / raw.
        let vdda_mv = VREFINT_MV * 4095 / sample;
        hprintln!(
            "gust-adc-silicon OK: Vrefint = {} raw on real {} silicon — the dissolved \
             adc-thin driver read the internal channel through the real ADC. From the \
             1.20 V nominal Vrefint this implies VDDA ≈ {} mV (VLDISCOVERY runs ~3.0 V; \
             band {}..{}, EOC cleared={}).",
            sample, BOARD, vdda_mv, VREF_LO, VREF_HI, eoc == 0
        );
        debug::exit(debug::EXIT_SUCCESS);
    } else {
        hprintln!(
            "gust-adc-silicon FAIL: Vrefint raw = {} outside {}..{} — internal channel \
             not converted (TSVREFE/clock/calibration issue).",
            sample, VREF_LO, VREF_HI
        );
        debug::exit(debug::EXIT_FAILURE);
    }
    loop {}
}
