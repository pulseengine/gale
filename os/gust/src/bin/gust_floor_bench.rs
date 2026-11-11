//! gust PROOF-CARRYING FLOOR bench — the measured 0.7× target (synth#494 part a).
//!
//! The companion to `gust_codegen_bench` (which measures dissolved-vs-native,
//! today **1.81×**). This bench measures the **floor** that proof-carrying
//! specialization unlocks — a number LLVM structurally cannot reach because it
//! lacks the range proof.
//!
//! `gust_mix` is `clamp(1500 + (ch-1024), 1000, 2000)`. When a composition proves
//! `ch ∈ [524,1524]` (a range gale primitives carry as a Verus/Rocq/Kani
//! invariant), `v = 1500 + (ch-1024) = ch + 476` is provably in [1000,2000] — BOTH
//! clamp branches are dead — and the whole function collapses to `add r0,#476`.
//! That is what synth COULD emit with the proof in hand; LLVM never will (it never
//! had the bound). `mix_proven` is that lowering.
//!
//! Three lowerings of the SAME mixer, all timed over the SAME proven-range inputs
//! under one SysTick / qemu `-icount` harness (deterministic; instr ≈ cycles on M3):
//!   1. `mix_native`   — full clamp, LLVM → thumbv7m         (what LLVM ships)
//!   2. `gust_mix`     — dissolved wasm→synth (linked .o)    (what we ship today)
//!   3. `mix_proven`   — `ch+476`, proof-carrying floor      (what synth COULD ship)
//!
//! Soundness gate: `mix_proven` is bit-identical to `mix_native` ONLY over the
//! proven range (asserted before timing) — that side-condition is the whole point.
//! Kept in a SEPARATE bin so it never perturbs the canonical `gust_codegen_bench`
//! layout/number.
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

// 1. native (LLVM) full-clamp mixer — same source as the dissolved gust_mix.
#[inline(never)]
fn mix_native(ch: u16) -> u16 {
    let v = 1500i32 + ((256i32 * (ch as i32 - 1024)) >> 8);
    (if v < 1000 { 1000 } else if v > 2000 { 2000 } else { v }) as u16
}

extern "C" {
    // 2. dissolved (synth) lowering of the SAME mixer (linked from wasm-kernel).
    fn gust_mix(ch: u16) -> u16;
}

// 3. proof-carrying floor: valid only for ch ∈ [PROVEN_LO, PROVEN_HI].
const PROVEN_LO: u16 = 524;
const PROVEN_HI: u16 = 1524;
#[inline(never)]
fn mix_proven(ch: u16) -> u16 {
    (ch as i32 + 476) as u16
}

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

    // ── soundness gate: mix_proven ≡ mix_native ≡ gust_mix over the PROVEN range ──
    let mut bad = 0u32;
    let mut ch = PROVEN_LO;
    loop {
        let n = mix_native(ch);
        if mix_proven(ch) != n || unsafe { gust_mix(ch) } != n {
            bad += 1;
        }
        if ch == PROVEN_HI {
            break;
        }
        ch += 1;
    }

    let _ = hprintln!("# gust FLOOR bench — proof-carrying specialization (synth#494a), the 0.7x target");
    let _ = hprintln!(
        "# soundness: mix_proven(ch)=ch+476 == mix_native == gust_mix over proven [524,1524] — {}",
        if bad == 0 { "sound" } else { "UNSOUND" }
    );
    let _ = hprintln!("# all lanes timed over the SAME proven-range inputs; native=1.0");
    let _ = hprintln!("gust-floor,fn,iters,total_ticks,milliticks_per_call");

    // all inputs inside the proven range so the premise holds for every lane
    let inputs: [u16; 4] = [524, 800, 1200, 1524];

    // baseline: loop + index + sink, NO call — subtracted to isolate the fn body
    let tb = now();
    for i in 0..ITERS {
        sink(inputs[(i & 3) as usize]);
    }
    let dt_base = delta(tb, now());

    let t0 = now();
    for i in 0..ITERS {
        sink(mix_native(inputs[(i & 3) as usize]));
    }
    let dt_native = delta(t0, now());

    let t1 = now();
    for i in 0..ITERS {
        sink(unsafe { gust_mix(inputs[(i & 3) as usize]) });
    }
    let dt_dissolved = delta(t1, now());

    let t2 = now();
    for i in 0..ITERS {
        sink(mix_proven(inputs[(i & 3) as usize]));
    }
    let dt_proven = delta(t2, now());

    let fn_native = dt_native.saturating_sub(dt_base);
    let fn_dissolved = dt_dissolved.saturating_sub(dt_base);
    let fn_proven = dt_proven.saturating_sub(dt_base);

    let mt = |t: u32| t as u64 * 1000 / ITERS as u64;
    let _ = hprintln!("gust-floor,baseline_loop,{},{},{}", ITERS, dt_base, mt(dt_base));
    let _ = hprintln!(
        "gust-floor,mix_native,{},{},{}  (fn-only {})",
        ITERS, dt_native, mt(dt_native), mt(fn_native)
    );
    let _ = hprintln!(
        "gust-floor,mix_dissolved_today,{},{},{}  (fn-only {})",
        ITERS, dt_dissolved, mt(dt_dissolved), mt(fn_dissolved)
    );
    let _ = hprintln!(
        "gust-floor,mix_proven_floor,{},{},{}  (fn-only {})",
        ITERS, dt_proven, mt(dt_proven), mt(fn_proven)
    );
    let ratio = |a: u32| -> u64 {
        if fn_native > 0 { a as u64 * 1000 / fn_native as u64 } else { 0 }
    };
    // dissolved/native today (the gap synth is closing) and proven/native (the floor)
    let _ = hprintln!("gust-floor,dissolved_today_ratio_x1000,,,{}", ratio(fn_dissolved));
    let _ = hprintln!("gust-floor,proof_carrying_floor_ratio_x1000,,,{}", ratio(fn_proven));
    let _ = hprintln!("gust-floor: done");

    debug::exit(if bad == 0 {
        debug::EXIT_SUCCESS
    } else {
        debug::EXIT_FAILURE
    });
    loop {}
}
