//! gust on RISC-V — the dissolved gust_mix (synth -b riscv --target esp32c3,
//! RV32IMC) driven by an esp-hal TCB on a real ESP32-C3. The THIRD architecture
//! after Cortex-M3 (F100) and Cortex-M4 (G474RE): same wasm source, same dissolve
//! pipeline, now lowered to RISC-V and run on Espressif silicon.
//!
//! Measures native (LLVM riscv32imc) vs dissolved (synth riscv) gust_mix in real
//! cycles via the RISC-V `mcycle` CSR (the M3/M4 used DWT CYCCNT). Output over the
//! USB-Serial-JTAG; flash + capture with `espflash flash --monitor` (the cargo
//! runner) or `cargo run --release`.
#![no_std]
#![no_main]

use esp_backtrace as _;
use esp_println::println;

extern "C" {
    fn gust_mix(ch: u16) -> u16; // dissolved (synth riscv esp32c3) — linked via build.rs
}

#[inline(never)]
fn mix_native(ch: u16) -> u16 {
    let v = 1500i32 + ((256i32 * (ch as i32 - 1024)) >> 8);
    (if v < 1000 { 1000 } else if v > 2000 { 2000 } else { v }) as u16
}

// ESP32-C3's RISC-V core does not implement the standard `mcycle` CSR (reading it
// traps), so time via the 16 MHz systimer instead. Absolute cycles are lost, but
// the native-vs-dissolved RATIO — the codegen-quality number — is preserved on a
// common time base. (High ITERS gives the ratio enough systimer-tick resolution.)
#[inline(always)]
fn ticks() -> u32 {
    esp_hal::time::now().duration_since_epoch().ticks() as u32
}

// volatile sink so the loops aren't optimized away
#[inline(always)]
fn sink(v: u16) {
    unsafe { core::ptr::read_volatile(&v) };
}

const ITERS: u32 = 200_000;

#[esp_hal::main]
fn main() -> ! {
    let _p = esp_hal::init(esp_hal::Config::default());
    println!("gust-esp32c3: RISC-V (ESP32-C3, systimer) — native (LLVM) vs dissolved (synth) gust_mix");

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
    println!(
        "# correctness: {} over [0,2047]",
        if mismatch == 0 { "IDENTICAL ok" } else { "MISMATCH FAIL" }
    );

    let inputs: [u16; 4] = [200, 1024, 1900, 2047];

    // Re-measure + re-print in a loop (crude busy delay) so a serial reader
    // catches the result whenever it attaches.
    loop {
        let b0 = ticks();
        for i in 0..ITERS { sink(inputs[(i & 3) as usize]); }
        let base = ticks().wrapping_sub(b0);

        let n0 = ticks();
        for i in 0..ITERS { sink(mix_native(inputs[(i & 3) as usize])); }
        let nat = ticks().wrapping_sub(n0).wrapping_sub(base);

        let d0 = ticks();
        for i in 0..ITERS { sink(unsafe { gust_mix(inputs[(i & 3) as usize]) }); }
        let dis = ticks().wrapping_sub(d0).wrapping_sub(base);

        let nat_mc = (nat as u64) * 1000 / ITERS as u64;
        let dis_mc = (dis as u64) * 1000 / ITERS as u64;
        let ratio = if nat > 0 { (dis as u64) * 1000 / nat as u64 } else { 0 };
        println!("gust-esp32c3,gust_mix_native,milliticks_per_call,{}", nat_mc);
        println!("gust-esp32c3,gust_mix_dissolved,milliticks_per_call,{}", dis_mc);
        println!("gust-esp32c3,ratio_x1000,,{} (mismatch={})", ratio, mismatch);
        println!("gust-esp32c3: done");
        for _ in 0..8_000_000u32 { unsafe { core::arch::asm!("nop") } }
    }
}
