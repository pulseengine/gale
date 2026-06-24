//! silicon_bench — on-SILICON cycle measurement of the dissolved gale code,
//! one bare-metal TCB that runs on BOTH the NUCLEO-G474RE (Cortex-M4) and the
//! STM32VLDISCOVERY / STM32F100 (Cortex-M3). True hardware cycles via the DWT
//! cycle counter (CYCCNT) — not qemu `-icount`.
//!
//! Same gust_mix as gust_codegen_bench: native (LLVM→thumb) vs dissolved
//! (wasm→loom→synth, linked from wasm-kernel/gust_mix-cm3.o, cortex-m3 — runs
//! unmodified on the M4 too). Correctness-gated over [0,2047] before timing.
//!
//! Build/flash/capture per board: benches/gust/silicon/run.sh {g474re|f100}
//! (selects the target + memory map, flashes via probe-rs, captures semihosting).
//! The DWT cycle counter is identical on M3 and M4, so the two boards' numbers
//! are directly comparable; the dissolved object is the same on both (the gust /
//! F100 cortex-m3 target), so the only variable is the silicon.
#![no_std]
#![no_main]
use cortex_m::peripheral::{Peripherals, DWT};
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use panic_halt as _;

// native (LLVM) Q8 mixer — identical source to gust_codegen_bench / the dissolved.
#[inline(never)]
fn mix_native(ch: u16) -> u16 {
    let v = 1500i32 + ((256i32 * (ch as i32 - 1024)) >> 8);
    (if v < 1000 { 1000 } else if v > 2000 { 2000 } else { v }) as u16
}

extern "C" {
    fn gust_mix(ch: u16) -> u16; // dissolved (synth), cortex-m3 .o — runs on M3 + M4
}

#[inline(always)]
fn sink(v: u16) {
    unsafe { core::ptr::read_volatile(&v) };
}

const ITERS: u32 = 20_000;

#[entry]
fn main() -> ! {
    let mut cp = Peripherals::take().unwrap();
    // Enable the DWT cycle counter (DEMCR.TRCENA via DCB, then DWT.CYCCNT).
    cp.DCB.enable_trace();
    cp.DWT.enable_cycle_counter();

    let _ = hprintln!("silicon_bench: DWT cycle counter — native (LLVM) vs dissolved (synth) gust_mix");

    // correctness gate over the full input domain
    let mut mismatch = 0u32;
    let mut ch = 0u16;
    loop {
        if mix_native(ch) != unsafe { gust_mix(ch) } {
            mismatch += 1;
        }
        if ch == 2047 { break; }
        ch += 1;
    }
    let _ = hprintln!(
        "# correctness: {} over [0,2047]",
        if mismatch == 0 { "IDENTICAL ok" } else { "MISMATCH FAIL" }
    );

    let inputs: [u16; 4] = [200, 1024, 1900, 2047];

    // baseline (loop + sink, no call) to isolate the function-body cycles
    let b0 = DWT::cycle_count();
    for i in 0..ITERS { sink(inputs[(i & 3) as usize]); }
    let base = DWT::cycle_count().wrapping_sub(b0);

    let n0 = DWT::cycle_count();
    for i in 0..ITERS { sink(mix_native(inputs[(i & 3) as usize])); }
    let nat = DWT::cycle_count().wrapping_sub(n0).wrapping_sub(base);

    let d0 = DWT::cycle_count();
    for i in 0..ITERS { sink(unsafe { gust_mix(inputs[(i & 3) as usize]) }); }
    let dis = DWT::cycle_count().wrapping_sub(d0).wrapping_sub(base);

    // milli-cycles per call (×1000 / ITERS) for sub-cycle resolution
    let nat_mc = (nat as u64) * 1000 / ITERS as u64;
    let dis_mc = (dis as u64) * 1000 / ITERS as u64;
    let ratio = if nat > 0 { (dis as u64) * 1000 / nat as u64 } else { 0 };
    let _ = hprintln!("silicon_bench,gust_mix_native,millicycles_per_call,{}", nat_mc);
    let _ = hprintln!("silicon_bench,gust_mix_dissolved,millicycles_per_call,{}", dis_mc);
    let _ = hprintln!("silicon_bench,ratio_x1000,,{}", ratio);
    let _ = hprintln!("silicon_bench: done");

    debug::exit(if mismatch == 0 { debug::EXIT_SUCCESS } else { debug::EXIT_FAILURE });
    loop {}
}
