//! gust:hal **thin-seam** UART driver — the maximal-wasm extreme.
//!
//! The ENTIRE STM32 USART protocol (init, baud, TXE/RXNE polling, RX drain)
//! lives here, in verified wasm. It imports only the generic `gust:hal/mmio`
//! (read32/write32) and `gust:hal/irq` (poll) capabilities; the trusted bridge
//! is a ~10-line generic register-poke + IRQ-flag, shared by every driver. No
//! host UART driver exists — this *is* the driver, dissolved to native.
//!
//! REQ-DRV-COMPONENT-001: this is a wasm **component** — `world uart-driver`
//! (`../wit/gust-hal.wit`), importing `gust:hal/mmio` + `gust:hal/irq` and exporting
//! `gust:hal/uart`. Both capabilities are therefore checked against a typed contract
//! at composition time, instead of being untyped `env` externs that only had to
//! match by name at native link.
//!
//! Build:  cargo build --release --target wasm32-unknown-unknown
//!         wasm-tools component new <wasm> -o uart.component.wasm
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

// wit-bindgen's canonical-ABI glue must LINK against a global allocator; this world
// is scalar-only (u32 in, u32 out), so nothing ever calls it and a zero-state
// trapping allocator keeps the driver's 0-SRAM property intact.
#[cfg(not(kani))]
use core::alloc::{GlobalAlloc, Layout};
#[cfg(not(kani))]
struct NoAlloc;
#[cfg(not(kani))]
unsafe impl GlobalAlloc for NoAlloc {
    unsafe fn alloc(&self, _: Layout) -> *mut u8 {
        core::ptr::null_mut()
    }
    unsafe fn dealloc(&self, _: *mut u8, _: Layout) {}
}
#[cfg(not(kani))]
#[global_allocator]
static ALLOC: NoAlloc = NoAlloc;

// gust:hal capability imports — now WIT-typed component imports (still the SAME
// three primitives the ~10-line TCB bridge supplies: mmio.{read32,write32},
// irq.poll).
#[cfg(not(kani))]
wit_bindgen::generate!({ world: "uart-driver", path: "../wit", generate_all });
#[cfg(not(kani))]
use crate::gust::hal::irq::poll as irq_poll;
#[cfg(not(kani))]
use crate::gust::hal::mmio::{read32, write32};

// Kani model-checks the pure RX decision core on the host, where no wasm bindings
// exist. No harness reaches an mmio/irq call (they were undefined `extern` symbols
// before this seam change, equally unreachable), so stubs stand in for the imports.
#[cfg(kani)]
fn read32(_addr: u32) -> u32 {
    0
}
#[cfg(kani)]
fn write32(_addr: u32, _val: u32) {}
#[cfg(kani)]
fn irq_poll(_line: u32) -> bool {
    false
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
    read32(a)
}
#[inline(always)]
fn wr(a: u32, v: u32) {
    write32(a, v)
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
///
/// `gust:hal/irq.poll` is declared `-> bool`, so the fired flag crosses the seam as
/// a boolean and this export normalises it to 0/1. Before componentization the raw
/// `env.irq_poll` extern returned `u32` and this forwarded the bridge's word
/// verbatim; every bridge in-tree already answers 0 or 1, and the interface doc has
/// always been "nonzero if the line fired", so no caller observes the difference —
/// but the value domain is now narrowed by the contract rather than by convention.
#[no_mangle]
pub extern "C" fn uart_rx_fired() -> u32 {
    irq_poll(RX_IRQ_LINE) as u32
}

// `gust:hal/uart` exported over the SAME bodies as the C-ABI symbols above, not a
// second implementation: the component's exports and the dissolved object's `uart_*`
// entry points (benches/gust/build.rs, gust_uart demonstrator) then cannot diverge
// in behaviour. Same shape gpio-thin used.
#[cfg(not(kani))]
struct Driver;
#[cfg(not(kani))]
impl exports::gust::hal::uart::Guest for Driver {
    fn init(brr: u32) {
        uart_init(brr)
    }
    fn tx_byte(b: u32) {
        uart_tx_byte(b)
    }
    fn rx() -> u32 {
        uart_rx()
    }
    fn rx_fired() -> u32 {
        uart_rx_fired()
    }
}
#[cfg(not(kani))]
export!(Driver);
