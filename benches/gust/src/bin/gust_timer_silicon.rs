//! gust-timer-silicon — REAL-HARDWARE anchor for the dissolved timer-thin driver.
//!
//! Like GPIO, every timer evidence in the tree is a **RAM-window** gate
//! (`//:gust-timer-renode`, `//:gust-breadth-renode`): those map TIM2 as plain
//! memory, so a counter read is just "the value someone wrote there" — a driver
//! whose `timer_now` returned a frozen register would pass. Nothing has ever shown
//! `timer_now` reading a counter that *moves on its own*. This firmware points the
//! dissolved timer-thin object at the REAL STM32F100 TIM2 and requires the counter
//! to advance. Flash + capture:  benches/gust/silicon/run-timer-f100.sh
//!
//! Self-checking over semihosting; no external wiring and nothing to observe.
//!
//! What is asserted (all must hold, or the run reports FAIL):
//!   1. **The counter advances.** Four `timer_now` samples, spaced by a short spin,
//!      must be strictly increasing. If all four are IDENTICAL the counter is not
//!      running — almost certainly TIM2 unclocked — and that is reported as a
//!      FAILURE, never as a pass.
//!   2. The driver's `timer_now(TIM2_BASE)` agrees with a direct read of the
//!      generated absolute `TIM2_CNT` taken immediately after (same counter, so the
//!      direct read must be ≥ the driver read and close to it).
//!   3. `timer_deadline` + `timer_elapsed` actually fire: a deadline set `TICKS`
//!      ahead becomes elapsed within a bounded spin, and is NOT already elapsed at
//!      the instant it is set.
//!   4. `timer_ack` clears a real hardware update flag: after the counter wraps ARR,
//!      SR.UIF is set by the hardware, and `timer_ack` must clear it.
//!
//! 16-BIT COUNTER CAVEAT (honest scope): TIM2 on the F1 is a **16-bit** counter, so
//! `timer_now` returns 0..=ARR. timer-thin's Kani-proven wrap-safety
//! (`has_elapsed`, u32 signed-difference) is a property of a **32-bit** counter and
//! does NOT hold across a 16-bit ARR reload — a deadline straddling a reload will
//! not fire. This run therefore evidences the deadline path only *within one ARR
//! period* (the sequence is sized to stay inside one) and does not exercise, let
//! alone confirm, the wrap-safety proof on hardware.
//!
//! LINKED OBJECT — honesty note: `drivers/timer-thin/timer-thin-cm3.o` as committed
//! is the pre-component dissolve (imports the untyped `mmio_read32`/`mmio_write32`
//! externs; timer-thin's source has not been componentized at all yet, unlike
//! gpio-thin). This firmware bridges the names that object actually imports. It
//! evidences the dissolved timer-thin *logic* on silicon, not a componentized build.
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

// The dissolved timer-thin driver (scalar ABI, 0 SRAM, 0 linmem).
extern "C" {
    fn timer_init(base: u32, psc: u32, arr: u32);
    fn timer_now(base: u32) -> u32;
    fn timer_deadline(now: u32, ticks: u32) -> u32;
    fn timer_elapsed(now: u32, deadline: u32) -> u32;
    fn timer_ack(base: u32);
}

// TIM2_BASE / TIM2_CNT / RCC_CSR are GENERATED from the AADL model
// (benches/gust/targets/stm32f100.aadl → targets/generated/) — see gust-target-gen.
// The STM32 timer register map is family-specific, so this firmware is F100-only.
#[cfg(not(feature = "target-f100"))]
compile_error!("gust_timer_silicon: build with --no-default-features --features target-f100 (the TIM2 map + APB1 clock tree are F1-specific)");
#[cfg(feature = "target-f100")]
#[path = "../../targets/generated/gust_target_stm32f100.rs"]
#[allow(dead_code)]
mod target;
use target::{BOARD, RCC_CSR, TIM2_BASE, TIM2_CNT};

// Build-time agreement between the generated model and the offset timer-thin
// compiles in (drivers/timer-thin/src/lib.rs: CNT = 0x24).
const _: () = assert!(TIM2_CNT == TIM2_BASE + 0x24);

// ---- MODEL GAP (report this, do not treat as a local constant) ----------------
// TIM2 lives on APB1, and the AADL `Rcc` device models only `Csr_Offset` (0x24) and
// `Apb2enr_Offset` (0x18) — there is **no** `Apb1enr_Offset` and no per-peripheral
// enable bit, so `RCC_APB1ENR` and `TIM2EN` are NOT generated. Rather than hardcode
// 0x4002_101C, the address is DERIVED from the generated `RCC_CSR` using the RCC
// base the model already fixes; the two numbers below (0x24, 0x1C) are exactly what
// the model is missing, and they are reported at runtime so any capture carries the
// gap. What the model would need, on `Rcc`:
//     Gust_Target_Props::Apb1enr_Offset => 16#1C#;
//     Gust_Target_Props::Tim2en_Bit     => 0;
// emitted as `RCC_APB1ENR` and `RCC_APB1ENR_TIM2EN`.
const RCC_CSR_OFFSET: u32 = 0x24; // modelled (Rcc::Csr_Offset) but not emitted alone
const RCC_BASE: u32 = RCC_CSR - RCC_CSR_OFFSET;
const APB1ENR_OFFSET_GAP: u32 = 0x1C; // <- NOT in the model
const RCC_APB1ENR: u32 = RCC_BASE + APB1ENR_OFFSET_GAP;
const TIM2EN_BIT_GAP: u32 = 0; // <- NOT in the model
const RCC_APB1ENR_TIM2EN: u32 = 1 << TIM2EN_BIT_GAP;
// The model likewise carries no TIM2 `Sr_Offset`; SR is read here only to observe
// the hardware update flag that `timer_ack` clears (the driver writes it itself).
// It would need `Gust_Target_Props::Sr_Offset => 16#10#;` on `Tim2`.
const TIM2_SR_OFFSET_GAP: u32 = 0x10;
const TIM2_SR: u32 = TIM2_BASE + TIM2_SR_OFFSET_GAP;
const UIF: u32 = 1 << 0;
// -------------------------------------------------------------------------------

// At reset the F100 runs on the 8 MHz HSI with AHB/APB1 prescalers = 1, so
// TIM2CLK = 8 MHz. PSC = 0 keeps the tick at 8 MHz and, crucially, avoids relying on
// the prescaler shadow register (which only loads at the first update event).
// ARR = 0xFFFF is the full 16-bit range → a wrap every 65536/8 MHz ≈ 8.19 ms, so the
// SR.UIF check below completes quickly. The clock tree is itself un-modelled: the
// AADL has no HSI/prescaler facts, so no tick-to-time conversion is claimed here.
const PSC: u32 = 0;
const ARR: u32 = 0xFFFF;

const SAMPLES: usize = 4;
const SPACING_NOPS: u32 = 200; // ≈ a few hundred ticks between samples
const DEADLINE_TICKS: u32 = 4_000; // ≈ 500 µs @ 8 MHz — well inside one ARR period
const SPIN_MAX: u32 = 4_000_000; // ≫ one full ARR period; bounds every wait

#[inline]
fn rd(addr: u32) -> u32 {
    unsafe { read_volatile(addr as *const u32) }
}
#[inline]
fn wr(addr: u32, val: u32) {
    unsafe { write_volatile(addr as *mut u32, val) }
}
#[inline(always)]
fn spin(n: u32) {
    for _ in 0..n {
        cortex_m::asm::nop();
    }
}

#[entry]
fn main() -> ! {
    hprintln!(
        "gust-timer-silicon: reading REAL {} TIM2 @0x{:08x} (CNT @0x{:08x}) through the \
         dissolved timer-thin driver. MODEL GAP: RCC_APB1ENR/TIM2EN are not generated \
         (no Rcc::Apb1enr_Offset, no Tim2en_Bit) — derived here as RCC_BASE(0x{:08x}) + \
         0x{:02x}, bit {}. See the source for what the AADL needs.",
        BOARD, TIM2_BASE, TIM2_CNT, RCC_BASE, APB1ENR_OFFSET_GAP, TIM2EN_BIT_GAP
    );

    // --- bring-up the driver does not model: the TIM2 peripheral clock (APB1) ---
    wr(RCC_APB1ENR, rd(RCC_APB1ENR) | RCC_APB1ENR_TIM2EN);

    // --- configure + start the timer THROUGH the driver ---
    unsafe { timer_init(TIM2_BASE, PSC, ARR) };

    // --- 1. does the counter move on its own? ---
    let mut s = [0u32; SAMPLES];
    for i in 0..SAMPLES {
        s[i] = unsafe { timer_now(TIM2_BASE) };
        spin(SPACING_NOPS);
    }
    let all_same = s.iter().all(|&v| v == s[0]);
    let mut monotonic = true;
    for i in 1..SAMPLES {
        if s[i] <= s[i - 1] {
            monotonic = false;
        }
    }

    // --- 2. the driver's read agrees with the generated absolute CNT ---
    let via_driver = unsafe { timer_now(TIM2_BASE) };
    let direct = rd(TIM2_CNT);
    let agree = direct >= via_driver && direct.wrapping_sub(via_driver) < 1_000;

    // --- 3. deadline / elapsed actually fire (inside one ARR period) ---
    let start = unsafe { timer_now(TIM2_BASE) };
    let dl = unsafe { timer_deadline(start, DEADLINE_TICKS) };
    let early = unsafe { timer_elapsed(start, dl) }; // must be 0 at the instant it is set
    let mut fired = false;
    let mut waited = 0u32;
    while waited < SPIN_MAX {
        if unsafe { timer_elapsed(timer_now(TIM2_BASE), dl) } == 1 {
            fired = true;
            break;
        }
        waited += 1;
    }
    let at_fire = unsafe { timer_now(TIM2_BASE) };

    // --- 4. timer_ack clears a REAL hardware update flag (set by an ARR wrap) ---
    let mut wrapped = false;
    let mut w = 0u32;
    while w < SPIN_MAX {
        if rd(TIM2_SR) & UIF != 0 {
            wrapped = true;
            break;
        }
        w += 1;
    }
    let uif_before = rd(TIM2_SR) & UIF;
    unsafe { timer_ack(TIM2_BASE) };
    let uif_after = rd(TIM2_SR) & UIF;
    let ack_ok = wrapped && uif_before != 0 && uif_after == 0;

    hprintln!(
        "gust-timer-silicon: samples {} {} {} {} (all_same={}, monotonic={}) | \
         timer_now={} direct CNT@0x{:08x}={} (agree={}) | deadline: start={} dl={} \
         early={} fired={} at={} | SR.UIF before ack={} after ack={} (wrapped={})",
        s[0], s[1], s[2], s[3], all_same, monotonic,
        via_driver, TIM2_CNT, direct, agree,
        start, dl, early, fired, at_fire, uif_before, uif_after, wrapped
    );

    if all_same {
        hprintln!(
            "gust-timer-silicon FAIL: TIM2_CNT read {} on all {} samples — the counter is \
             NOT running. Most likely TIM2 is unclocked: this firmware set RCC_APB1ENR \
             @0x{:08x} bit {} from an UN-MODELLED offset (the AADL has no \
             Rcc::Apb1enr_Offset / Tim2en_Bit), so that derivation is the first suspect. \
             A frozen counter is a FAILURE, not a pass.",
            s[0], SAMPLES, RCC_APB1ENR, TIM2EN_BIT_GAP
        );
        debug::exit(debug::EXIT_FAILURE);
    } else if monotonic && agree && early == 0 && fired && ack_ok {
        hprintln!(
            "gust-timer-silicon OK: on real {} silicon the dissolved timer-thin driver \
             started TIM2 and read a counter that advances by itself ({} -> {} across {} \
             samples); the driver read matches the generated CNT address; a {}-tick \
             deadline was not elapsed when set and fired on its own; and timer_ack \
             cleared a hardware-set SR.UIF. Scope: TIM2 is 16-bit, so the u32 \
             wrap-safety proof is NOT exercised here.",
            BOARD, s[0], s[SAMPLES - 1], SAMPLES, DEADLINE_TICKS
        );
        debug::exit(debug::EXIT_SUCCESS);
    } else {
        hprintln!(
            "gust-timer-silicon FAIL: monotonic={} agree={} early={} (want 0) fired={} \
             ack_ok={} (wrapped={} uif_before={} uif_after={})",
            monotonic, agree, early, fired, ack_ok, wrapped, uif_before, uif_after
        );
        debug::exit(debug::EXIT_FAILURE);
    }
    loop {}
}
