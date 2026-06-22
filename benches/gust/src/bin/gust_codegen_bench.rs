//! gust codegen micro-bench — native (LLVM→thumbv7m) vs dissolved (wasm→loom→synth→cm3),
//! the SAME source function, timed under one SysTick harness (qemu `-icount`,
//! deterministic; true M3 cycles via Renode). This is the per-function *cycle*
//! companion to the *size* table in COMPARE.md — the apples-to-apples evidence
//! for driving synth/loom codegen (synth#390 / loom#226).
//!
//! `gust_mix` is the fair micro-target: identical Q8 fixed-point mixer, pure
//! scalar in/out (no pointer, no scheduler state, no r11 trampoline). One copy is
//! lowered by LLVM (the Rust `mix` below), the other by synth (the dissolved
//! `gust_mix` linked from wasm-kernel/gust_kernel-cortex-m3.o). Both are checked
//! bit-identical before timing, then each is run over the same input sweep that
//! exercises all three clamp branches (<1000, in-range, >2000).
#![no_std]
#![no_main]
use cortex_m::peripheral::syst::SystClkSource;
use cortex_m::peripheral::Peripherals;
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use panic_halt as _;

#[inline(always)]
fn now() -> u32 {
    cortex_m::peripheral::SYST::get_current()
}
#[inline(always)]
fn delta(a: u32, b: u32) -> u32 {
    a.wrapping_sub(b) & 0x00FF_FFFF
}

// The native (LLVM) lowering of the Q8 failsafe mixer — same source as
// benches/gust/src/main.rs and the dissolved gust_mix.
#[inline(never)]
fn mix_native(ch: u16) -> u16 {
    let v = 1500i32 + ((256i32 * (ch as i32 - 1024)) >> 8);
    (if v < 1000 { 1000 } else if v > 2000 { 2000 } else { v }) as u16
}

extern "C" {
    // The dissolved (synth) lowering of the SAME mixer. Pure scalar fn — called
    // directly (no native-pointer-abi / r11 trampoline needed).
    fn gust_mix(ch: u16) -> u16;
}

// Volatile sink so the optimizer cannot elide the timed loops.
#[inline(always)]
fn sink(v: u16) {
    unsafe { core::ptr::read_volatile(&v) };
}

const ITERS: u32 = 20_000;

#[entry]
fn main() -> ! {
    let cp = Peripherals::take().unwrap();
    let mut syst = cp.SYST;
    syst.set_clock_source(SystClkSource::Core);
    syst.set_reload(0x00FF_FFFF);
    syst.clear_current();
    syst.enable_counter();

    // ── correctness gate: native ≡ dissolved across the full input domain ──
    let mut mismatch = 0u32;
    let mut ch = 0u16;
    loop {
        if mix_native(ch) != unsafe { gust_mix(ch) } {
            mismatch += 1;
        }
        if ch == 2047 {
            break;
        }
        ch += 1;
    }
    let _ = hprintln!("# gust codegen bench — native (LLVM) vs dissolved (synth) gust_mix");
    let _ = hprintln!(
        "# correctness: {} over [0,2047] — {}",
        if mismatch == 0 { "IDENTICAL" } else { "MISMATCH" },
        if mismatch == 0 { "ok" } else { "FAIL" }
    );
    let _ = hprintln!("# ticks = SysTick under qemu -icount (deterministic); ratio is codegen quality, LLVM=1.0");
    let _ = hprintln!("gust-codegen,fn,iters,total_ticks,milliticks_per_call");

    // sweep inputs hitting all three clamp branches, identical for both sides
    let inputs: [u16; 4] = [200, 1024, 1900, 2047];

    // ── baseline: the loop + index + volatile sink, NO mix call ──
    // Subtracting this isolates the function-body cycles (the codegen delta),
    // since the native and dissolved loops carry identical harness overhead.
    let tb = now();
    for i in 0..ITERS {
        sink(inputs[(i & 3) as usize]);
    }
    let dt_base = delta(tb, now());

    // ── native ──
    let t0 = now();
    for i in 0..ITERS {
        sink(mix_native(inputs[(i & 3) as usize]));
    }
    let dt_native = delta(t0, now());

    // ── dissolved ──
    let t1 = now();
    for i in 0..ITERS {
        sink(unsafe { gust_mix(inputs[(i & 3) as usize]) });
    }
    let dt_dissolved = delta(t1, now());

    // function-only ticks = measured − baseline
    let fn_native = dt_native.saturating_sub(dt_base);
    let fn_dissolved = dt_dissolved.saturating_sub(dt_base);

    let _ = hprintln!(
        "gust-codegen,baseline_loop,{},{},{}",
        ITERS, dt_base, (dt_base as u64 * 1000 / ITERS as u64)
    );
    let _ = hprintln!(
        "gust-codegen,mix_native,{},{},{}  (fn-only {})",
        ITERS, dt_native, (dt_native as u64 * 1000 / ITERS as u64),
        (fn_native as u64 * 1000 / ITERS as u64)
    );
    let _ = hprintln!(
        "gust-codegen,mix_dissolved,{},{},{}  (fn-only {})",
        ITERS, dt_dissolved, (dt_dissolved as u64 * 1000 / ITERS as u64),
        (fn_dissolved as u64 * 1000 / ITERS as u64)
    );
    // function-only cycle ratio ×1000 (dissolved / native), the codegen-quality number
    let ratio_milli = if fn_native > 0 {
        fn_dissolved as u64 * 1000 / fn_native as u64
    } else {
        0
    };
    let _ = hprintln!("gust-codegen,fn_only_ratio_x1000,,,{}", ratio_milli);
    let _ = hprintln!("gust-codegen: done");

    debug::exit(if mismatch == 0 {
        debug::EXIT_SUCCESS
    } else {
        debug::EXIT_FAILURE
    });
    loop {}
}
