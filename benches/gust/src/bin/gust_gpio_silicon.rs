//! gust-gpio-silicon — REAL-HARDWARE anchor for the dissolved gpio-thin driver.
//!
//! Every GPIO evidence the tree holds today is a **RAM-window** gate: `gust_gpio`
//! (Renode) and `gust_gpio_probe`-class probes map GPIOC as plain memory, so they
//! pin the *register writes* the driver makes but never the *electrical* effect —
//! nothing has ever confirmed that a BSRR write from this driver actually moves a
//! pin. This firmware closes that: it points the dissolved gpio-thin object at the
//! REAL STM32F100 GPIOC and drives the STM32VLDISCOVERY's on-board LEDs, reading
//! the level back **through the pin** (IDR) rather than through the register the
//! driver just wrote. Flash + capture:  benches/gust/silicon/run-gpio-f100.sh
//!
//! Self-checking over semihosting — nobody has to watch an LED. PC8 (LD4, blue) and
//! PC9 (LD3, green) are used because they are electrically safe (each drives an LED
//! through a series resistor to ground, no external wiring) *and* visually
//! confirmable as a bonus, but the pass/fail verdict comes entirely from IDR.
//!
//! What is asserted (all four must hold, or the run reports FAIL):
//!   1. `gpio_configure` really landed — the pin's 4-bit CRH field reads back as the
//!      output-push-pull-50MHz nibble `0x3`. A silently no-op'd configure (the
//!      table-in-linmem failure mode gpio-thin's header warns about) is caught here.
//!   2. `gpio_set` → `gpio_read` (IDR) == 1, and `gpio_clear` → `gpio_read` == 0 on
//!      PC8. Reading the same value in both directions = FAIL (stuck / unclocked
//!      port / pin not actually driven), so this cannot false-pass.
//!   3. `gpio_toggle` on PC9 flips the pin low→high→low, each step confirmed by IDR.
//!   4. Cross-pin independence: driving PC9 leaves PC8 where PC8 was left.
//!
//! LINKED OBJECT — honesty note: `drivers/gpio-thin/gpio-thin-cm3.o` as committed
//! predates gpio-thin's componentization (gale#245): its imports are still the
//! untyped `mmio_read32`/`mmio_write32` externs, not the `gust:hal/mmio` field names
//! `read32`/`write32` that a component dissolve emits (compare wdg-thin-cm3.o). This
//! firmware therefore bridges the names the committed object actually imports. It
//! exercises the dissolved gpio-thin *logic* on silicon; it does **not** evidence the
//! componentized build until that object is regenerated.
//!
//! F1 bring-up this driver does not model (done here, one-time): the GPIOC port
//! clock (RCC APB2ENR IOPCEN) — see the MODEL GAP note below.
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

// The dissolved gpio-thin driver (scalar ABI, 0 SRAM, 0 linmem).
extern "C" {
    fn gpio_configure(port_base: u32, pin: u32, mode_idx: u32);
    fn gpio_set(port_base: u32, pin: u32);
    fn gpio_clear(port_base: u32, pin: u32);
    fn gpio_read(port_base: u32, pin: u32) -> u32;
    fn gpio_toggle(port_base: u32, pin: u32);
}

// GPIOC_BASE / GPIOC_CRH / GPIOC_BSRR / RCC_APB2ENR are GENERATED from the AADL
// model (benches/gust/targets/stm32f100.aadl → targets/generated/) — see
// gust-target-gen. The STM32F1 GPIO register map is family-specific, so this
// firmware is F100-only (thumbv7m), like gust_adc_silicon.
#[cfg(not(feature = "target-f100"))]
compile_error!("gust_gpio_silicon: build with --no-default-features --features target-f100 (the STM32F1 CRL/CRH/BSRR map is F1-specific)");
#[cfg(feature = "target-f100")]
#[path = "../../targets/generated/gust_target_stm32f100.rs"]
#[allow(dead_code)]
mod target;
use target::{BOARD, GPIOC_BASE, GPIOC_BSRR, GPIOC_CRH, RCC_APB2ENR};

// Build-time agreement between the generated model and the offsets gpio-thin
// compiles in (drivers/gpio-thin/src/lib.rs: CRH=0x04, BSRR=0x10). If the model and
// the driver ever disagree about where a register lives, this fails the build rather
// than mis-poking silicon.
const _: () = assert!(GPIOC_CRH == GPIOC_BASE + 0x04);
const _: () = assert!(GPIOC_BSRR == GPIOC_BASE + 0x10);

// ---- MODEL GAP (report this, do not treat as a local constant) ----------------
// The AADL `Rcc` device models `Base`, `Csr_Offset`, `Apb2enr_Offset`,
// `Iwdgrstf_Bit` and `Rmvf_Bit` — but NOT the per-peripheral *enable bits* inside
// APB2ENR. `RCC_APB2ENR` is generated; the GPIOC enable bit (IOPCEN, bit 2 of the
// IOPxEN group = bit 4 of APB2ENR) is not, so it is declared here and reported at
// runtime. What the model would need: `Gust_Target_Props::Iopcen_Bit => 4;` on
// `Rcc` (mirroring the existing `Iwdgrstf_Bit`/`Rmvf_Bit` style), emitted as
// `RCC_APB2ENR_IOPCEN`. gust_adc_silicon has the same gap for ADC1EN.
const RCC_APB2ENR_IOPCEN_BIT: u32 = 4;
const RCC_APB2ENR_IOPCEN: u32 = 1 << RCC_APB2ENR_IOPCEN_BIT;
// The model also does not describe *which* pins carry the board's LEDs (a board
// fact, not a SoC fact): PC8 = LD4 (blue), PC9 = LD3 (green) on STM32VLDISCOVERY.
// It would need a board-level property, e.g. `Gust_Target_Props::Led_Pins`.
const PIN_BLUE: u32 = 8; // LD4
const PIN_GREEN: u32 = 9; // LD3
// -------------------------------------------------------------------------------

// gpio-thin mode index 4 → nibble 0x3 = MODE=11 (output 50 MHz), CNF=00 (push-pull).
// Same index gust_gpio (the Renode content-gate) uses, so the two agree on the mode.
const MODE_OUT_PP50: u32 = 4;
const NIB_OUT_PP50: u32 = 0x3;

#[inline]
fn rd(addr: u32) -> u32 {
    unsafe { read_volatile(addr as *const u32) }
}
#[inline]
fn wr(addr: u32, val: u32) {
    unsafe { write_volatile(addr as *mut u32, val) }
}
/// Let the pin settle before sampling IDR (IDR is resampled on the APB2 clock; the
/// LED's series RC is negligible, but a push-pull edge still needs a cycle or two).
#[inline(always)]
fn settle() {
    for _ in 0..64 {
        cortex_m::asm::nop();
    }
}
/// The 4-bit CRH config field of a pin in 8..=15.
#[inline]
fn crh_nibble(crh: u32, pin: u32) -> u32 {
    (crh >> ((pin - 8) * 4)) & 0xF
}

#[entry]
fn main() -> ! {
    hprintln!(
        "gust-gpio-silicon: driving REAL {} GPIOC @0x{:08x} (PC{} = LD4 blue, PC{} = \
         LD3 green) through the dissolved gpio-thin driver; every level is read back \
         from the PIN via IDR, not from the register just written. MODEL GAP: the \
         GPIOC port-clock bit (RCC_APB2ENR bit {}) is not generated — see the source.",
        BOARD, GPIOC_BASE, PIN_BLUE, PIN_GREEN, RCC_APB2ENR_IOPCEN_BIT
    );

    // --- bring-up the driver does not model: the GPIOC port clock ---
    wr(RCC_APB2ENR, rd(RCC_APB2ENR) | RCC_APB2ENR_IOPCEN);

    // --- 1. configure both pins as push-pull outputs THROUGH the driver ---
    unsafe {
        gpio_configure(GPIOC_BASE, PIN_BLUE, MODE_OUT_PP50);
        gpio_configure(GPIOC_BASE, PIN_GREEN, MODE_OUT_PP50);
    }
    let crh = rd(GPIOC_CRH);
    let nib_blue = crh_nibble(crh, PIN_BLUE);
    let nib_green = crh_nibble(crh, PIN_GREEN);
    let cfg_ok = nib_blue == NIB_OUT_PP50 && nib_green == NIB_OUT_PP50;

    // --- 2. set / clear PC8 through the driver, read the PIN back each time ---
    unsafe { gpio_set(GPIOC_BASE, PIN_BLUE) };
    settle();
    let blue_hi = unsafe { gpio_read(GPIOC_BASE, PIN_BLUE) };
    unsafe { gpio_clear(GPIOC_BASE, PIN_BLUE) };
    settle();
    let blue_lo = unsafe { gpio_read(GPIOC_BASE, PIN_BLUE) };

    // --- 3. toggle PC9 twice (ODR-driven, BSRR-applied), read the PIN back ---
    unsafe { gpio_toggle(GPIOC_BASE, PIN_GREEN) };
    settle();
    let green_1 = unsafe { gpio_read(GPIOC_BASE, PIN_GREEN) };
    unsafe { gpio_toggle(GPIOC_BASE, PIN_GREEN) };
    settle();
    let green_2 = unsafe { gpio_read(GPIOC_BASE, PIN_GREEN) };

    // --- 4. cross-pin independence: PC8 was left low and must still be low ---
    let blue_after = unsafe { gpio_read(GPIOC_BASE, PIN_BLUE) };

    let drive_ok = blue_hi == 1 && blue_lo == 0;
    let toggle_ok = green_1 == 1 && green_2 == 0;
    let indep_ok = blue_after == 0;

    hprintln!(
        "gust-gpio-silicon: CRH=0x{:08x} (PC{} nibble=0x{:x}, PC{} nibble=0x{:x}, want \
         0x{:x}) | PC{} set->IDR={} clear->IDR={} | PC{} toggle->IDR={} toggle->IDR={} \
         | PC{} after={}",
        crh, PIN_BLUE, nib_blue, PIN_GREEN, nib_green, NIB_OUT_PP50,
        PIN_BLUE, blue_hi, blue_lo, PIN_GREEN, green_1, green_2, PIN_BLUE, blue_after
    );

    if cfg_ok && drive_ok && toggle_ok && indep_ok {
        hprintln!(
            "gust-gpio-silicon OK: on real {} silicon the dissolved gpio-thin driver \
             configured PC{}/PC{} as push-pull outputs, and every BSRR set/clear/toggle \
             moved the PHYSICAL pin — confirmed by reading IDR back (1 then 0, and \
             0->1->0 for the toggle), with the other pin undisturbed. LD4/LD3 also \
             blinked, but the verdict is the IDR readback.",
            BOARD, PIN_BLUE, PIN_GREEN
        );
        debug::exit(debug::EXIT_SUCCESS);
    } else if !cfg_ok {
        hprintln!(
            "gust-gpio-silicon FAIL: gpio_configure did not land — CRH nibbles are \
             0x{:x}/0x{:x}, want 0x{:x}/0x{:x}. Either the port clock (RCC_APB2ENR bit \
             {}) is off or the driver's config write no-op'd.",
            nib_blue, nib_green, NIB_OUT_PP50, NIB_OUT_PP50, RCC_APB2ENR_IOPCEN_BIT
        );
        debug::exit(debug::EXIT_FAILURE);
    } else if blue_hi == blue_lo {
        hprintln!(
            "gust-gpio-silicon FAIL: PC{} read back {} both driven-high and driven-low \
             — the pin is stuck (not actually driven / IDR unclocked), so BSRR had no \
             electrical effect.",
            PIN_BLUE, blue_hi
        );
        debug::exit(debug::EXIT_FAILURE);
    } else {
        hprintln!(
            "gust-gpio-silicon FAIL: drive_ok={} toggle_ok={} indep_ok={} (hi={} lo={} \
             t1={} t2={} after={})",
            drive_ok, toggle_ok, indep_ok, blue_hi, blue_lo, green_1, green_2, blue_after
        );
        debug::exit(debug::EXIT_FAILURE);
    }
    loop {}
}
