//! gust-fused — the full-pipeline demonstrator.
//!
//! A Component-Model composition (gale-app-demo imports `gale:kernel`; gale-kiln
//! provides it over the verified `gale::*` decisions) is MELD-fused
//! (`--memory shared --address-rebase`) into one merged-memory core module,
//! loom-inlined, then synth-dissolved to a native Cortex-M3 relocatable object.
//! This bare-metal Rust TCB links that object and calls its `run-demo` export.
//!
//! No wasm runtime is present. The SAME composed component that runs on wasmtime
//! today (`crates/gale-app-demo/run.sh` → `run-demo() = 53`) dissolves to this
//! native image and produces the identical result here on the metal — the
//! "components on top, meld-fused down to a fused module, run on the gust stack"
//! story made executable.
//!
//! Build + boot:  `cargo run --release --bin gust_fused`  (qemu lm3s6965evb / M3)
//! Footprint:     text ~3.5 KB, bss 8 B — fits the 8 KB-SRAM node class
//!                (the dissolved kernel boots under an 8 KB Renode SRAM map; see
//!                renode-test/gust_m3_8k.repl).
#![no_std]
#![no_main]
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use panic_halt as _;

extern "C" {
    // run-demo -> packed sem/msgq/mutex decisions from the fused, dissolved
    // Component-Model composition. synth keeps the wasm export name verbatim
    // (hyphen and all), so the link name is "run-demo".
    #[link_name = "run-demo"]
    fn run_demo() -> u32;
}

#[entry]
fn main() -> ! {
    let _ = hprintln!(
        "gust-fused boot: CM composition (app + gale-kiln) meld-fused --memory shared -> loom -> synth -> native TCB"
    );
    let r = unsafe { run_demo() };
    // 53 = take(0,true)=would-block(1) | give(0,3,false)=increment(1)<<2
    //      | put(0,4,4,_,true)=full(3)<<4 — identical to the wasmtime run-demo().
    let _ = hprintln!("gust-fused: run-demo() = {} (expect 53)", r);
    debug::exit(if r == 53 {
        debug::EXIT_SUCCESS
    } else {
        debug::EXIT_FAILURE
    });
    loop {}
}
