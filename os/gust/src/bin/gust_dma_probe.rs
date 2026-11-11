//! gust-dma-probe — the LOCAL qemu-semihosting probe of the DISSOLVED dma-own .o,
//! run BEFORE the Renode content-gate to catch a dissolved no-op fast.
//!
//! dma-own is the DMA-as-`own<buffer>` OWNERSHIP round-trip driver (gale#124,
//! Kani 6/6): a transfer is modeled as `own<buffer>` moving from wasm to the DMA
//! agent and back on completion, with the cache/barrier op emitted *by
//! construction* at every handoff. Unlike the thin mmio drivers, its distinctive
//! property is the OWNERSHIP FSM itself: wasm is provably hands-off for the
//! transfer window, and the coherency barrier is paired with the transition (not
//! by convention). This probe drives the three dissolved exports —
//! `dma_start` (Wasm→Dma), `dma_poll_complete` (Dma→Wasm on IRQ), `dma_abort`
//! (→Wasm from any state) — supplying the trusted seam (`dma_program` /
//! `dma_barrier` / `dma_irq_poll`) as writes/reads over a plain `[u32; 8]` RAM
//! window (real mapped SRAM on lm3s6965evb), and asserts, per handoff, the EXACT
//! register effects THROUGH the seam plus the ownership property:
//!   * start: Wasm→Dma, clean+DSB emitted, descriptor programmed (ch/len);
//!   * double-arm while Dma-owned is REJECTED, no second program, no barrier
//!     (exclusive ownership — wasm cannot re-arm the in-flight buffer);
//!   * poll with no IRQ yields state unchanged, no barrier (split-phase);
//!   * poll with IRQ fired: Dma→Wasm, invalidate+DMB emitted (round-trip closes);
//!   * unpaired complete (from Wasm) is REJECTED, no barrier;
//!   * abort from any state returns to Wasm with invalidate+DMB — never ownerless.
//! A dissolved primitive that silently no-ops (e.g. returns its input state, or
//! skips the barrier/program seam call) fails HERE, on `cargo run`.
#![no_std]
#![no_main]
use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use panic_halt as _;

// DMA descriptor + coherency register window in RAM. The dissolved driver never
// writes these directly — it pokes them THROUGH the trusted seam below (a
// descriptor program + a barrier op), exactly as it would poke silicon. The
// probe reads them back to assert the exact effects of each ownership handoff.
// word 0=prog channel, 1=prog len, 2=prog count, 3=last barrier op,
//      4=barrier count, 5=IRQ pending (probe-controlled).
static mut REG: [u32; 8] = [0, 0, 0, BARRIER_NONE, 0, 0, 0, 0];
const R_CH: usize = 0;
const R_LEN: usize = 1;
const R_PROG_CNT: usize = 2;
const R_BAR: usize = 3;
const R_BAR_CNT: usize = 4;
const R_IRQ: usize = 5;

#[inline]
fn rd(i: usize) -> u32 {
    unsafe { read_volatile(addr_of!(REG[i])) }
}
#[inline]
fn wr(i: usize, v: u32) {
    unsafe { write_volatile(addr_of_mut!(REG[i]), v) }
}

// The gust:hal DMA seam the dissolved driver imports — the irreducible trusted
// atoms (descriptor poke, coherency barrier, completion-IRQ poll). Here they
// read/write the RAM window so the probe can observe them exactly.
#[no_mangle]
pub extern "C" fn dma_program(channel: u32, len: u32) {
    wr(R_CH, channel);
    wr(R_LEN, len);
    wr(R_PROG_CNT, rd(R_PROG_CNT) + 1);
}
#[no_mangle]
pub extern "C" fn dma_barrier(op: u32) {
    wr(R_BAR, op);
    wr(R_BAR_CNT, rd(R_BAR_CNT) + 1);
}
#[no_mangle]
pub extern "C" fn dma_irq_poll(_channel: u32) -> u32 {
    let p = rd(R_IRQ);
    wr(R_IRQ, 0); // fired-and-clears, like real IRQ status
    p
}

extern "C" {
    fn dma_start(state: u32, channel: u32, len: u32) -> u32;
    fn dma_poll_complete(state: u32, channel: u32) -> u32;
    fn dma_abort(state: u32, channel: u32) -> u32;
}

// Owner-state scalar ABI (see dma-own/src/lib.rs): 0=Wasm, 1=Dma.
const WASM: u32 = 0;
const DMA: u32 = 1;
const XFER_FAULT: u32 = 0xFFFF_FFFF;
// Barrier discriminants: CleanDsb=0 (Wasm→Dma), InvalidateDmb=1 (Dma→Wasm).
const CLEAN_DSB: u32 = 0;
const INVAL_DMB: u32 = 1;
const BARRIER_NONE: u32 = 0xFFFF_FFFF;
const CH: u32 = 2;
const LEN: u32 = 64;

#[entry]
fn main() -> ! {
    let mut ok = true;

    // 1) start: Wasm→Dma. The driver MUST emit clean+DSB (barrier op 0) BEFORE the
    //    engine reads memory, program the descriptor (ch/len) THROUGH the seam, and
    //    hand ownership to the DMA agent. A no-op that returns the input state
    //    (still Wasm) fails here.
    let s1 = unsafe { dma_start(WASM, CH, LEN) };
    let prog1 = rd(R_PROG_CNT);
    let ch1 = rd(R_CH);
    let len1 = rd(R_LEN);
    let bar1 = rd(R_BAR);
    let barcnt1 = rd(R_BAR_CNT);
    if s1 != DMA || prog1 != 1 || ch1 != CH || len1 != LEN || bar1 != CLEAN_DSB || barcnt1 != 1 {
        hprintln!(
            "dma-start FAIL: s1={:#x} want {:#x} prog={} ch={} len={} bar={:#x} want {:#x} barcnt={}",
            s1, DMA, prog1, ch1, len1, bar1, CLEAN_DSB, barcnt1
        );
        ok = false;
    } else {
        hprintln!(
            "dma-start ok: Wasm->Dma, clean+DSB emitted, descriptor programmed ch={} len={}",
            ch1, len1
        );
    }

    // 2) exclusive ownership: while Dma-owned, a second start (double-arm) MUST be
    //    rejected with XFER_FAULT — no new descriptor program, no new barrier. wasm
    //    is provably hands-off the in-flight buffer; it cannot re-arm it.
    let prog_before = rd(R_PROG_CNT);
    let barcnt_before = rd(R_BAR_CNT);
    let s_dbl = unsafe { dma_start(s1, CH, LEN) };
    let prog_after = rd(R_PROG_CNT);
    let barcnt_after = rd(R_BAR_CNT);
    if s_dbl != XFER_FAULT || prog_after != prog_before || barcnt_after != barcnt_before {
        hprintln!(
            "dma-exclusive FAIL: s_dbl={:#x} want {:#x} prog {}->{} barcnt {}->{}",
            s_dbl, XFER_FAULT, prog_before, prog_after, barcnt_before, barcnt_after
        );
        ok = false;
    } else {
        hprintln!("dma-exclusive ok: double-arm rejected while Dma-owned, no program, no barrier");
    }

    // 3) split-phase yield: poll with no completion IRQ leaves state unchanged
    //    (still Dma) and emits NO barrier — the kiln task just yields.
    wr(R_IRQ, 0);
    let barcnt_b = rd(R_BAR_CNT);
    let s_yield = unsafe { dma_poll_complete(s1, CH) };
    let barcnt_a = rd(R_BAR_CNT);
    if s_yield != DMA || barcnt_a != barcnt_b {
        hprintln!(
            "dma-yield FAIL: s_yield={:#x} want {:#x} barcnt {}->{}",
            s_yield, DMA, barcnt_b, barcnt_a
        );
        ok = false;
    } else {
        hprintln!("dma-yield ok: poll with no IRQ keeps Dma-owned, no barrier");
    }

    // 4) complete: with the completion IRQ fired, Dma→Wasm — the driver MUST emit
    //    invalidate+DMB (barrier op 1) AFTER the engine wrote, and re-own the
    //    buffer. This closes the ownership round-trip.
    wr(R_IRQ, 1);
    let s2 = unsafe { dma_poll_complete(s1, CH) };
    let bar2 = rd(R_BAR);
    let barcnt2 = rd(R_BAR_CNT);
    if s2 != WASM || bar2 != INVAL_DMB || barcnt2 != barcnt_a + 1 {
        hprintln!(
            "dma-complete FAIL: s2={:#x} want {:#x} bar={:#x} want {:#x} barcnt={}",
            s2, WASM, bar2, INVAL_DMB, barcnt2
        );
        ok = false;
    } else {
        hprintln!("dma-complete ok: Dma->Wasm on IRQ, invalidate+DMB emitted, buffer re-owned");
    }

    // 5) unpaired complete: a completion poll from Wasm (no outstanding transfer),
    //    even with the IRQ asserted, MUST be rejected with XFER_FAULT and emit no
    //    barrier — a completion without a start is a fault, not silent corruption.
    wr(R_IRQ, 1);
    let barcnt_u = rd(R_BAR_CNT);
    let s_bad = unsafe { dma_poll_complete(WASM, CH) };
    let barcnt_u2 = rd(R_BAR_CNT);
    if s_bad != XFER_FAULT || barcnt_u2 != barcnt_u {
        hprintln!(
            "dma-unpaired FAIL: s_bad={:#x} want {:#x} barcnt {}->{}",
            s_bad, XFER_FAULT, barcnt_u, barcnt_u2
        );
        ok = false;
    } else {
        hprintln!("dma-unpaired ok: completion from Wasm rejected, no barrier");
    }

    // 6) abort never ownerless: from a fresh Dma-owned transfer, abort MUST return
    //    the buffer to Wasm with invalidate+DMB; and abort from Wasm ALSO returns
    //    Wasm with a barrier. Total from every state — never a limbo/gap.
    let s_arm = unsafe { dma_start(WASM, CH, LEN) };
    let s_ab1 = unsafe { dma_abort(s_arm, CH) };
    let bar_ab1 = rd(R_BAR);
    let s_ab2 = unsafe { dma_abort(WASM, CH) };
    let bar_ab2 = rd(R_BAR);
    if s_arm != DMA
        || s_ab1 != WASM
        || bar_ab1 != INVAL_DMB
        || s_ab2 != WASM
        || bar_ab2 != INVAL_DMB
    {
        hprintln!(
            "dma-abort FAIL: s_arm={:#x} s_ab1={:#x} bar={:#x} s_ab2={:#x} bar={:#x}",
            s_arm, s_ab1, bar_ab1, s_ab2, bar_ab2
        );
        ok = false;
    } else {
        hprintln!("dma-abort ok: abort from Dma AND from Wasm both return Wasm w/ invalidate+DMB");
    }

    if ok {
        hprintln!("dma-probe ALL OK");
        debug::exit(debug::EXIT_SUCCESS);
    } else {
        debug::exit(debug::EXIT_FAILURE);
    }
    loop {}
}
