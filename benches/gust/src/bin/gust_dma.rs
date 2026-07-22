//! gust-dma — the DMA-as-`own<buffer>` ownership driver driven bare-metal on gust,
//! with a self-checking Renode content-gate (gust-OS driver breadth close-out).
//!
//! Links ONLY the dissolved dma-own driver (gale#124, Kani 6/6); the trusted DMA
//! seam (descriptor program / coherency barrier / completion-IRQ poll) and the raw
//! USART1 poke are plumbing to report results. The driver models a DMA transfer as
//! an ownership round-trip — `own<buffer>` moves wasm→engine on start and back on
//! completion, with the cache/barrier op emitted BY CONSTRUCTION at every handoff.
//! Asserts, on a RAM-mapped DMA descriptor + coherency window: (a) start hands the
//! buffer Wasm→Dma, emits clean+DSB, and programs the descriptor (ch/len); (b) a
//! double-arm while Dma-owned is rejected — no second program, no barrier
//! (exclusive ownership); (c) a poll with no IRQ yields state unchanged (split-
//! phase); (d) completion (IRQ fired) hands Dma→Wasm and emits invalidate+DMB
//! (round-trip closes); (e) an unpaired completion from Wasm is rejected; (f)
//! abort returns the buffer to Wasm from any state with invalidate+DMB — never
//! ownerless. Deterministic, no dependence on a Renode DMA-controller model.
#![no_std]
#![no_main]
use core::ptr::{read_volatile, write_volatile};
use cortex_m_rt::entry;
use cortex_m_semihosting::debug;
use panic_halt as _;

// RAM-mapped DMA descriptor + coherency register window (see gust_dma.repl). The
// dissolved driver pokes it THROUGH the trusted seam (program/barrier), exactly as
// it would poke silicon; the gate reads it back to assert exact effects.
const DMA: u32 = 0x4002_0000; // RAM-mapped DMA window in the gate .repl
const D_CH: u32 = 0x00;
const D_LEN: u32 = 0x04;
const D_PROG_CNT: u32 = 0x08;
const D_BAR: u32 = 0x0C;
const D_BAR_CNT: u32 = 0x10;
const D_IRQ: u32 = 0x14;

#[inline]
fn drd(off: u32) -> u32 {
    unsafe { read_volatile((DMA + off) as *const u32) }
}
#[inline]
fn dwr(off: u32, v: u32) {
    unsafe { write_volatile((DMA + off) as *mut u32, v) }
}

// gust:hal DMA seam — the 3 irreducible trusted atoms, here backed by the RAM
// window so the gate can observe each ownership handoff's exact effects.
#[no_mangle]
pub extern "C" fn dma_program(channel: u32, len: u32) {
    dwr(D_CH, channel);
    dwr(D_LEN, len);
    dwr(D_PROG_CNT, drd(D_PROG_CNT) + 1);
}
#[no_mangle]
pub extern "C" fn dma_barrier(op: u32) {
    dwr(D_BAR, op);
    dwr(D_BAR_CNT, drd(D_BAR_CNT) + 1);
}
#[no_mangle]
pub extern "C" fn dma_irq_poll(_channel: u32) -> u32 {
    let p = drd(D_IRQ);
    dwr(D_IRQ, 0);
    p
}

extern "C" {
    fn dma_start(state: u32, channel: u32, len: u32) -> u32;
    fn dma_poll_complete(state: u32, channel: u32) -> u32;
    fn dma_abort(state: u32, channel: u32) -> u32;
}

const WASM: u32 = 0;
const OWN_DMA: u32 = 1;
const XFER_FAULT: u32 = 0xFFFF_FFFF;
const CLEAN_DSB: u32 = 0;
const INVAL_DMB: u32 = 1;
const CH: u32 = 2;
const LEN: u32 = 64;

const USART1: u32 = 0x4001_3800;
const USART_SR: u32 = 0x00;
const USART_DR: u32 = 0x04;
const USART_BRR: u32 = 0x08;
const USART_CR1: u32 = 0x0C;
const TXE: u32 = 1 << 7;

fn tx(s: &[u8]) {
    for &b in s {
        unsafe {
            while read_volatile((USART1 + USART_SR) as *const u32) & TXE == 0 {}
            write_volatile((USART1 + USART_DR) as *mut u32, (b as u32) & 0xFF);
        }
    }
}

#[entry]
fn main() -> ! {
    unsafe {
        // enable GPIOA(PA9 TX), AFIO, USART1; PA9 → AF push-pull; USART1 8MHz/115200.
        const RCC_APB2ENR: u32 = 0x4002_1018;
        let e = read_volatile(RCC_APB2ENR as *const u32);
        write_volatile(RCC_APB2ENR as *mut u32, e | (1 << 0) | (1 << 2) | (1 << 14));
        const GPIOA_CRH: u32 = 0x4001_0804;
        let c = read_volatile(GPIOA_CRH as *const u32);
        write_volatile(GPIOA_CRH as *mut u32, (c & !(0xF << 4)) | (0xB << 4));
        write_volatile((USART1 + USART_BRR) as *mut u32, 0x45);
        write_volatile((USART1 + USART_CR1) as *mut u32, (1 << 13) | (1 << 3));

        // Coherency register starts empty; DMA descriptor window zeroed by SRAM.
        dwr(D_BAR, XFER_FAULT);

        tx(b"dma-gate begin\n");

        // 1) start: Wasm→Dma, emit clean+DSB, program the descriptor (ch/len).
        let s1 = dma_start(WASM, CH, LEN);
        let prog1 = drd(D_PROG_CNT);
        let ch1 = drd(D_CH);
        let len1 = drd(D_LEN);
        let bar1 = drd(D_BAR);
        tx(
            if s1 == OWN_DMA && prog1 == 1 && ch1 == CH && len1 == LEN && bar1 == CLEAN_DSB {
                b"dma-start-ok\n"
            } else {
                b"dma-start-bad\n"
            },
        );

        // 2) exclusive ownership: double-arm while Dma-owned is rejected — no new
        //    program, no new barrier.
        let prog_b = drd(D_PROG_CNT);
        let barcnt_b = drd(D_BAR_CNT);
        let s_dbl = dma_start(s1, CH, LEN);
        tx(
            if s_dbl == XFER_FAULT && drd(D_PROG_CNT) == prog_b && drd(D_BAR_CNT) == barcnt_b {
                b"dma-exclusive-ok\n"
            } else {
                b"dma-exclusive-bad\n"
            },
        );

        // 3) split-phase yield: poll with no IRQ keeps Dma-owned, no barrier.
        dwr(D_IRQ, 0);
        let barcnt_y = drd(D_BAR_CNT);
        let s_yield = dma_poll_complete(s1, CH);
        tx(if s_yield == OWN_DMA && drd(D_BAR_CNT) == barcnt_y {
            b"dma-yield-ok\n"
        } else {
            b"dma-yield-bad\n"
        });

        // 4) complete: IRQ fired → Dma→Wasm, emit invalidate+DMB. Round-trip closes.
        dwr(D_IRQ, 1);
        let barcnt_c = drd(D_BAR_CNT);
        let s2 = dma_poll_complete(s1, CH);
        let bar2 = drd(D_BAR);
        tx(
            if s2 == WASM && bar2 == INVAL_DMB && drd(D_BAR_CNT) == barcnt_c + 1 {
                b"dma-complete-ok\n"
            } else {
                b"dma-complete-bad\n"
            },
        );

        // 5) unpaired complete: completion from Wasm is rejected, no barrier.
        dwr(D_IRQ, 1);
        let barcnt_u = drd(D_BAR_CNT);
        let s_bad = dma_poll_complete(WASM, CH);
        tx(if s_bad == XFER_FAULT && drd(D_BAR_CNT) == barcnt_u {
            b"dma-unpaired-ok\n"
        } else {
            b"dma-unpaired-bad\n"
        });

        // 6) abort never ownerless: abort from Dma AND from Wasm both return Wasm
        //    with invalidate+DMB.
        let s_arm = dma_start(WASM, CH, LEN);
        let s_ab1 = dma_abort(s_arm, CH);
        let bar_ab1 = drd(D_BAR);
        let s_ab2 = dma_abort(WASM, CH);
        let bar_ab2 = drd(D_BAR);
        tx(
            if s_arm == OWN_DMA
                && s_ab1 == WASM
                && bar_ab1 == INVAL_DMB
                && s_ab2 == WASM
                && bar_ab2 == INVAL_DMB
            {
                b"dma-abort-ok\n"
            } else {
                b"dma-abort-bad\n"
            },
        );

        tx(b"dma-gate done\n");
    }
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}
