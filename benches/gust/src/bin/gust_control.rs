//! gust-control — north-star rung 1: a REALISTIC example driven on the gust stack.
//! The kiln-async scheduler (gust's executor) runs the dissolved engine_control
//! algorithm (synth-dissolved `control_step`, the same one the C bench / WIT
//! component use) as its task body — one control tick per scheduler round:
//! read a (simulated) crank sample → control_step → spark/fuel actuators.
//!
//! Where gust_stack drives the fixed-result `run-demo`, this drives a real
//! sensors→actuators control loop — the workload an engine node actually runs —
//! dissolved to native and scheduled by kiln, bare-metal, no wasm runtime.
//!
//! Boot: cargo run --release --bin gust_control  (qemu lm3s6965evb / Cortex-M3)
#![no_std]
#![no_main]
use core::sync::atomic::{AtomicU32, Ordering};
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use kiln_async::{SchedConfig, Scheduler, TaskOutcome};
use panic_halt as _;

// The dissolved control_step (synth --native-pointer-abi) reads its ignition/fuel
// tables relative to the wasm linmem base, which the ABI pins to r11 == 0. The
// kiln scheduler uses r11 as a normal callee-saved register, so calls from inside
// poll_round must re-zero it first. This 5-instruction r11=0 trampoline is the TCB
// shim (same pattern as the dissolved gust kernel); control_step_packed_body is the
// renamed synth symbol.
core::arch::global_asm!(
    ".section .text.control_step_packed",
    ".global control_step_packed",
    ".thumb_func",
    "control_step_packed:",
    "    push  {{r11, lr}}",
    "    mov.w r11, #0",
    "    bl    control_step_packed_body",
    "    pop   {{r11, pc}}",
);

extern "C" {
    // dissolved engine_control via the r11=0 trampoline above: (spark as u16)<<16 | fuel.
    // Same algorithm as benches/engine_control (control.c / the WIT component).
    fn control_step_packed(rpm: u32, load_pct: u32, coolant_c: i32, knock: u32) -> u32;
}

type S = Scheduler<8, 8, 4, 2, 2>;
static TICK: AtomicU32 = AtomicU32::new(0);
static LAST: AtomicU32 = AtomicU32::new(0);

#[entry]
fn main() -> ! {
    let _ = hprintln!(
        "gust-control: kiln-async scheduler driving the dissolved engine_control loop (sensors -> control_step -> actuators)"
    );

    // correctness gate: one known operating point (3000 rpm, 50% load, 80C, no knock)
    let known = unsafe { control_step_packed(3000, 50, 80, 0) };
    let ok = known == ((33u32 << 16) | 2300); // spark 33deg, fuel 2300us — == wasmtime/C
    let _ = hprintln!(
        "# control_step(3000,50,80,0) = spark {}deg fuel {}us — {}",
        (known >> 16) as i16, known & 0xFFFF,
        if ok { "matches C/wasmtime ok" } else { "MISMATCH" }
    );

    let mut s = S::new(SchedConfig::DEFAULT);
    let _ = s.spawn();

    const TICKS: u32 = 5000;
    for _ in 0..TICKS {
        let _ = s.poll_round(|_sched, _id, _fuel| {
            // one control tick: synthesize a crank sample (rpm ramps 800..8800,
            // load tracks rpm, warm engine, no knock) and run the dissolved step.
            let t = TICK.fetch_add(1, Ordering::Relaxed);
            let rpm = 800 + (t % 80) * 100;     // 800..8700 rpm sweep
            let load = 10 + (t % 18) * 5;        // 10..95 %
            let act = unsafe { control_step_packed(rpm, load, 80, 0) };
            LAST.store(act, Ordering::Relaxed);
            Ok(TaskOutcome::Yielded)
        });
    }

    let last = LAST.load(Ordering::Relaxed);
    let _ = hprintln!(
        "gust-control: {} control ticks on the kiln/gust stack; last actuators = spark {}deg fuel {}us",
        TICK.load(Ordering::Relaxed), (last >> 16) as i16, last & 0xFFFF
    );
    let _ = hprintln!(
        "gust-control: dissolved engine_control ran as scheduled work on gust (native, no runtime)"
    );
    debug::exit(if ok { debug::EXIT_SUCCESS } else { debug::EXIT_FAILURE });
    loop {}
}
