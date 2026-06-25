//! gust:hal **thin-seam** UART driver — the maximal-wasm extreme.
//!
//! The ENTIRE STM32 USART protocol (init, baud, TXE/RXNE polling, RX drain)
//! lives here, in verified wasm. It imports only the generic `gust:hal/mmio`
//! (read32/write32) and `gust:hal/irq` (poll) capabilities; the trusted bridge
//! is a ~10-line generic register-poke + IRQ-flag, shared by every driver. No
//! host UART driver exists — this *is* the driver, dissolved to native.
//!
//! Build:  cargo build --release --target wasm32-unknown-unknown
//! Dissolve: loom optimize --passes inline | synth compile --target cortex-m3
//!           --native-pointer-abi --shadow-stack-size <n> --all-exports --relocatable
// no_std for the wasm32 dissolve target; under `cargo kani` we build for the host
// (std) so the model checker can exercise the pure decision logic.
#![cfg_attr(not(kani), no_std)]

#[cfg(not(kani))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

// gust:hal capability imports — become import-call relocations in the dissolved
// object, resolved at link by the TCB bridge (mmio.{read32,write32}, irq.poll).
extern "C" {
    fn mmio_read32(addr: u32) -> u32;
    fn mmio_write32(addr: u32, val: u32);
    /// irq.poll(line): nonzero if the line fired since last poll (and clears it).
    fn irq_poll(line: u32) -> u32;
}

// STM32F1 USART1 register map — the only device knowledge, and it is *data*
// (addresses/bitmasks), not trusted code. F100 value line is compatible here.
const USART1: u32 = 0x4001_3800;
const SR: u32 = USART1 + 0x00; // status
const DR: u32 = USART1 + 0x04; // data (low 9 bits)
const BRR: u32 = USART1 + 0x08; // baud divisor
const CR1: u32 = USART1 + 0x0C; // control 1

const TXE: u32 = 1 << 7; // transmit data register empty
const RXNE: u32 = 1 << 5; // read data register not empty
const ORE: u32 = 1 << 3; // overrun error
const FE: u32 = 1 << 1; // framing error
const UE: u32 = 1 << 13; // USART enable
const TE: u32 = 1 << 3; // transmitter enable
const RE: u32 = 1 << 2; // receiver enable

/// USART RX status decision — the driver's pure, verifiable core (gale `_decide`
/// style). Total over all SR values; **errors take priority over data-ready** so
/// the driver never reads DR on an overrun/framing error (which would desync the
/// byte stream — the safety property). Proven by Kani here; the Verus + Rocq
/// tracks attach when this is promoted into a gale verified module / its buffering
/// reuses the already-proven gale::msgq ring (see REQ-DRV-VERIFY-001).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RxStatus {
    Idle,
    Ready,
    Overrun,
    FramingError,
}

#[inline]
pub fn usart_rx_decide(sr: u32) -> RxStatus {
    if sr & ORE != 0 {
        RxStatus::Overrun
    } else if sr & FE != 0 {
        RxStatus::FramingError
    } else if sr & RXNE != 0 {
        RxStatus::Ready
    } else {
        RxStatus::Idle
    }
}

const RX_IRQ_LINE: u32 = 0;

#[inline(always)]
fn rd(a: u32) -> u32 {
    unsafe { mmio_read32(a) }
}
#[inline(always)]
fn wr(a: u32, v: u32) {
    unsafe { mmio_write32(a, v) }
}

/// Sentinel returned by `uart_rx` when no byte is available (or an error gated
/// the read) — keeps the export a plain scalar, no linmem/option in the ABI.
pub const RX_NONE: u32 = 0xFFFF_FFFF;

// ---- exported protocol primitives (the driver's gust:hal-facing surface) ----
// A driver provides primitives; the app owns the payload. This keeps the driver
// free of any data segment (no embedded strings) → 0 linmem, 0 SRAM, and no
// native-pointer-abi data-placement dependency.

#[no_mangle]
pub extern "C" fn uart_init(brr: u32) {
    wr(BRR, brr);
    wr(CR1, UE | TE | RE);
}

#[no_mangle]
pub extern "C" fn uart_tx_byte(b: u32) {
    while rd(SR) & TXE == 0 {}
    wr(DR, b & 0xFF);
}

/// Read one byte if available — gated on the *verified* decision: only read DR on
/// Ready, never on an error (reading mid-error would desync the stream). Returns
/// RX_NONE when Idle/error.
#[no_mangle]
pub extern "C" fn uart_rx() -> u32 {
    match usart_rx_decide(rd(SR)) {
        RxStatus::Ready => rd(DR) & 0xFF,
        _ => RX_NONE,
    }
}

/// Kani proofs for the verifiable core (`cargo kani`). Totality + the
/// error-priority safety property.
#[cfg(kani)]
mod kani_proofs {
    use super::*;

    /// Over ALL status-register values: decide is total (no panic), never says
    /// Ready while an error bit is set, and Ready implies RXNE with no errors.
    #[kani::proof]
    fn rx_decide_error_priority() {
        let sr: u32 = kani::any();
        let d = usart_rx_decide(sr);
        if (sr & ORE != 0) || (sr & FE != 0) {
            assert!(d != RxStatus::Ready); // never read DR on error
        }
        if d == RxStatus::Ready {
            assert!(sr & RXNE != 0 && sr & ORE == 0 && sr & FE == 0);
        }
    }
}

/// Split-phase RX availability check — does the bridge ISR report the RX line
/// fired? Lets the driver yield to kiln between bytes rather than spin. Exposed
/// so the app can drive the split-phase loop (start → yield → poll).
#[no_mangle]
pub extern "C" fn uart_rx_fired() -> u32 {
    unsafe { irq_poll(RX_IRQ_LINE) }
}
