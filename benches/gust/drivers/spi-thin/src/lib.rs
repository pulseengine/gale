//! gust:hal **thin-seam** SPI driver — v0.3.0 driver breadth, the first RTIO
//! iodev (FIND-DRV-RTIO-001). The SQE→CQE submission/completion shape mapped onto
//! the gust:hal seam: a transfer is *submitted* (`spi_begin`), *stepped* per byte,
//! and *completes* when the last byte lands — the same request/complete lifecycle
//! io_uring/RTIO uses, here as a Kani-proven state machine.
//!
//! The ENTIRE STM32F1 (F100 value-line) SPI protocol — the CR1 mode/baud encoding
//! (CPOL/CPHA/BR), master-mode + software-NSS setup, and the full-duplex byte
//! shift-in/shift-out over SR/DR — lives here, in verified wasm. It imports ONLY
//! `gust:hal/mmio` (read32/write32) — the SAME subset gpio-thin uses — so it adds
//! **zero new TCB atoms**. No host SPI driver exists; this *is* the driver,
//! dissolved to native.
//!
//! Build:   cargo build --release --target wasm32-unknown-unknown
//! Dissolve: loom optimize --passes inline | synth compile --target cortex-m3
//!           --all-exports --relocatable
//! Verify:  cargo kani   (the pure transfer FSM: exclusive bus + no lost byte)
#![cfg_attr(not(kani), no_std)]

#[cfg(not(kani))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

// gust:hal mmio capability — becomes import-call relocations resolved at link by
// the SAME ~10-line TCB bridge gpio-thin/uart-thin use (mmio.{read32,write32}).
// No irq/dma atom: a thin-seam polled SPI needs only register reads/writes.
extern "C" {
    fn mmio_read32(addr: u32) -> u32;
    fn mmio_write32(addr: u32, val: u32);
}

// STM32F1 SPI register map (offsets from the peripheral base, e.g. SPI1=0x4001_3000).
// Device knowledge as *data* (offsets/bit math), not trusted code.
const CR1: u32 = 0x00; // control 1 (mode, baud, enable)
const SR: u32 = 0x08; // status  (RXNE bit0, TXE bit1, BSY bit7)
const DR: u32 = 0x0C; // data    (write to TX, read to RX)

// CR1 field placement (STM32F1 reference manual). CPHA=bit0 / CPOL=bit1 are the
// `mode & 0b11` bits placed directly by `mode_bits`, so they need no named const.
const CR1_MSTR: u32 = 1 << 2; // master mode
const CR1_SPE: u32 = 1 << 6; // SPI enable
const CR1_SSI: u32 = 1 << 8; // internal slave-select (high in SSM master)
const CR1_SSM: u32 = 1 << 9; // software slave management
const BR_SHIFT: u32 = 3; // baud-rate divisor field BR[5:3]

const SR_RXNE: u32 = 1 << 0; // receive buffer not empty
const SR_TXE: u32 = 1 << 1; // transmit buffer empty

// ───────────────────── pure config core (table-free) ─────────────────────
//
// CRITICAL — TABLE-FREE by construction (see gpio-thin): a `match`/array from an
// index to a value compiles to a `.rodata` linear-memory lookup table, which a
// driver dissolved `--relocatable` (no linmem base) reads as 0 → silent no-op.
// The mode/baud encoding is therefore pure bit arithmetic — no table, no linmem.

/// SPI mode 0..=3 → the CPOL/CPHA bits, by definition `mode = (CPOL<<1)|CPHA`.
/// So the two config bits ARE `mode & 0b11` placed at CR1[1:0] — pure arithmetic,
/// no lookup. Modes >3 are masked to their low 2 bits (can never set a stray bit).
#[inline]
pub fn mode_bits(mode: u32) -> u32 {
    // CPHA = bit0, CPOL = bit1 — SPI mode's bit layout is identical to CR1[1:0]
    // (mode 0=(0,0) … mode 3=(1,1)), so the config bits ARE `mode & 0b11`.
    mode & 0b11
}

/// Baud divisor index 0..=7 → the CR1 BR[5:3] field. `br_idx` selects f_PCLK/2^(n+1);
/// masked to 3 bits so it can never overflow the field. Pure shift, no table.
#[inline]
pub fn baud_bits(br_idx: u32) -> u32 {
    (br_idx & 0b111) << BR_SHIFT
}

/// The full CR1 value for a master-mode, software-NSS transfer at `mode`/`br_idx`.
/// Deterministic function of its inputs — this is exactly what the Renode
/// content-gate asserts on the RAM-mapped SPI window.
#[inline]
pub fn cr1_value(mode: u32, br_idx: u32) -> u32 {
    CR1_SPE | CR1_MSTR | CR1_SSM | CR1_SSI | baud_bits(br_idx) | mode_bits(mode)
}

// ───────────────────── transfer FSM (the RTIO iodev core) ─────────────────────
//
// The SQE→CQE lifecycle as a proven state machine, mirroring dma-own's ownership
// round-trip. A transfer is submitted with a byte count, steps once per shifted
// byte, and completes when the count reaches zero. `Phase` IS the state — there is
// no separate "in flight" limbo, keeping "exclusive bus" trivially checkable.

/// Where a transfer is in its lifecycle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// No transfer in flight — the bus is free; a new submit may begin.
    Idle,
    /// A transfer is in flight; `remaining` bytes still to shift. Exclusive: no
    /// second transfer may begin until this reaches Idle (complete or abort).
    Active,
    /// The submitted transfer finished — every byte shifted (remaining == 0).
    Complete,
}

/// Why a transition was rejected. A rejected submit/step never corrupts the FSM.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// `begin` while a transfer is already in flight (bus not free), or a
    /// zero-length submit (nothing to transfer).
    BusBusy,
    /// `step` when no transfer is Active (a completion pulse with nothing in flight).
    NotActive,
}

/// A transfer's state: its phase plus how many bytes are still to shift. The pair
/// is the whole state; `remaining` is meaningful only while `Active`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Xfer {
    pub phase: Phase,
    pub remaining: u32,
}

impl Xfer {
    /// The free bus — no transfer in flight.
    pub const fn idle() -> Self {
        Xfer { phase: Phase::Idle, remaining: 0 }
    }
}

/// Bus-free predicate: a new transfer may be submitted **iff** the bus is Idle.
/// This is the exclusivity property the design guarantees (one master, one
/// in-flight transfer at a time).
#[inline]
pub const fn bus_free(x: Xfer) -> bool {
    matches!(x.phase, Phase::Idle)
}

/// Submit a transfer of `count` bytes. Succeeds only from Idle with count > 0
/// (exclusive bus). Returns the Active state with `remaining = count`.
#[inline]
pub const fn begin(x: Xfer, count: u32) -> Result<Xfer, Fault> {
    match x.phase {
        Phase::Idle if count > 0 => Ok(Xfer { phase: Phase::Active, remaining: count }),
        _ => Err(Fault::BusBusy),
    }
}

/// Shift one byte: from Active, decrement `remaining` by exactly one; the transfer
/// becomes Complete iff that was the last byte (remaining hits 0), else stays
/// Active. Rejected unless Active — a step can never lose or invent a byte.
#[inline]
pub const fn step(x: Xfer) -> Result<Xfer, Fault> {
    match x.phase {
        Phase::Active => {
            let rem = x.remaining - 1;
            if rem == 0 {
                Ok(Xfer { phase: Phase::Complete, remaining: 0 })
            } else {
                Ok(Xfer { phase: Phase::Active, remaining: rem })
            }
        }
        _ => Err(Fault::NotActive),
    }
}

/// Abort / teardown: return the bus to Idle from ANY state — never leaves a
/// transfer wedged Active. Total; defined for every state.
#[inline]
pub const fn abort(_x: Xfer) -> Xfer {
    Xfer::idle()
}

// ───────────────────────────── dissolve ABI ─────────────────────────────────
//
// Scalar in/out, no linmem/data segment → 0 SRAM, no native-pointer dependency.
// State is carried by the caller as a packed u32 (phase in the top 2 bits,
// remaining in the low 30) so the whole FSM crosses the seam without a pointer.

/// Sentinel for a rejected FSM transition (fault) — keeps the ABI scalar, same
/// idiom as dma-own's `XFER_FAULT` / uart-thin's `RX_NONE`.
pub const SPI_FAULT: u32 = 0xFFFF_FFFF;

const PHASE_SHIFT: u32 = 30;
const REM_MASK: u32 = (1 << PHASE_SHIFT) - 1; // low 30 bits

#[inline]
fn ph_enc(p: Phase) -> u32 {
    match p {
        Phase::Idle => 0,
        Phase::Active => 1,
        Phase::Complete => 2,
    }
}
#[inline]
fn ph_dec(b: u32) -> Phase {
    match b {
        0 => Phase::Idle,
        1 => Phase::Active,
        _ => Phase::Complete,
    }
}
/// Pack an `Xfer` into the scalar carried across the dissolve seam.
#[inline]
fn pack(x: Xfer) -> u32 {
    (ph_enc(x.phase) << PHASE_SHIFT) | (x.remaining & REM_MASK)
}
/// Unpack the scalar back into an `Xfer`.
#[inline]
fn unpack(s: u32) -> Xfer {
    Xfer { phase: ph_dec(s >> PHASE_SHIFT), remaining: s & REM_MASK }
}

#[inline(always)]
fn rd(a: u32) -> u32 {
    unsafe { mmio_read32(a) }
}
#[inline(always)]
fn wr(a: u32, v: u32) {
    unsafe { mmio_write32(a, v) }
}

// ---- exported protocol primitives (the driver's gust:hal-facing surface) ----

/// Configure the SPI peripheral at `base` for master mode at `mode` (0..=3) and
/// baud index `br_idx` (0..=7). Writes the computed CR1 in one store (also enables
/// the peripheral). Table-free: the value is pure bit arithmetic.
#[no_mangle]
pub extern "C" fn spi_configure(base: u32, mode: u32, br_idx: u32) {
    wr(base + CR1, cr1_value(mode, br_idx));
}

/// Full-duplex shift of one byte: wait for TXE, write `out` to DR, wait for RXNE,
/// return the received byte. The polled I/O that the FSM's `step` accounts for.
#[no_mangle]
pub extern "C" fn spi_xfer_byte(base: u32, out: u32) -> u32 {
    while rd(base + SR) & SR_TXE == 0 {}
    wr(base + DR, out & 0xFF);
    while rd(base + SR) & SR_RXNE == 0 {}
    rd(base + DR) & 0xFF
}

/// Submit a transfer of `count` bytes (SQE). Returns the packed Active state or
/// `SPI_FAULT` (bus busy / zero length). Pure FSM — pairs with the byte loop.
#[no_mangle]
pub extern "C" fn spi_begin(state: u32, count: u32) -> u32 {
    match begin(unpack(state), count) {
        Ok(next) => pack(next),
        Err(_) => SPI_FAULT,
    }
}

/// Account one shifted byte (advance toward the CQE). Returns the packed next
/// state — Complete once the last byte lands — or `SPI_FAULT` if not Active.
#[no_mangle]
pub extern "C" fn spi_step(state: u32) -> u32 {
    match step(unpack(state)) {
        Ok(next) => pack(next),
        Err(_) => SPI_FAULT,
    }
}

/// True (1) once the submitted transfer has completed (CQE ready), else 0.
#[no_mangle]
pub extern "C" fn spi_is_complete(state: u32) -> u32 {
    matches!(unpack(state).phase, Phase::Complete) as u32
}

/// Abort the transfer: return the bus to Idle from any state. Never faults.
#[no_mangle]
pub extern "C" fn spi_abort(state: u32) -> u32 {
    pack(abort(unpack(state)))
}

// ─────────────────────────────── Kani proofs ────────────────────────────────
//
// The two safety properties over the full input space. Run: `cargo kani`.
#[cfg(kani)]
mod kani_proofs {
    use super::*;

    fn any_phase() -> Phase {
        match kani::any::<u8>() % 3 {
            0 => Phase::Idle,
            1 => Phase::Active,
            _ => Phase::Complete,
        }
    }
    fn any_xfer() -> Xfer {
        Xfer { phase: any_phase(), remaining: kani::any() }
    }

    /// P1 — exclusive bus: `begin` succeeds IFF the bus is Idle and the count is
    /// nonzero; a submit onto a busy bus is always rejected and never mutates.
    #[kani::proof]
    fn p1_exclusive_bus() {
        let x = any_xfer();
        let count: u32 = kani::any();
        match begin(x, count) {
            Ok(next) => {
                assert_eq!(x.phase, Phase::Idle);
                assert!(count > 0);
                assert_eq!(next.phase, Phase::Active);
                assert_eq!(next.remaining, count);
            }
            Err(f) => {
                assert!(x.phase != Phase::Idle || count == 0);
                assert_eq!(f, Fault::BusBusy);
            }
        }
        // bus_free tracks Idle exactly.
        assert_eq!(bus_free(x), x.phase == Phase::Idle);
    }

    /// P2 — no lost byte: each Active `step` decrements `remaining` by exactly one,
    /// and the transfer reports Complete IFF that decrement reached zero. Over a
    /// full run of N steps this shifts exactly N bytes — none lost, none invented.
    #[kani::proof]
    fn p2_no_lost_byte() {
        let rem: u32 = kani::any();
        kani::assume(rem > 0); // Active carries at least one outstanding byte
        let x = Xfer { phase: Phase::Active, remaining: rem };
        let next = step(x).unwrap();
        // exactly one byte accounted
        assert_eq!(next.remaining, rem - 1);
        // Complete iff that was the last byte
        assert_eq!(next.phase == Phase::Complete, rem == 1);
        assert_eq!(next.phase == Phase::Active, rem > 1);
    }

    /// P2b — step is rejected unless Active (a completion pulse with nothing in
    /// flight can't drive the counter negative or resurrect a finished transfer).
    #[kani::proof]
    fn p2b_step_requires_active() {
        let x = any_xfer();
        kani::assume(x.phase != Phase::Active);
        assert_eq!(step(x), Err(Fault::NotActive));
    }

    /// P3 — abort is total and always frees the bus (never wedged Active).
    #[kani::proof]
    fn p3_abort_frees_bus() {
        let x = any_xfer();
        let next = abort(x);
        assert_eq!(next.phase, Phase::Idle);
        assert!(bus_free(next));
    }

    /// P4 — pack/unpack round-trips every reachable state (the scalar ABI carries
    /// the FSM losslessly across the dissolve seam).
    #[kani::proof]
    fn p4_pack_roundtrip() {
        let x = any_xfer();
        kani::assume(x.remaining <= REM_MASK);
        let y = unpack(pack(x));
        assert_eq!(y.phase, x.phase);
        assert_eq!(y.remaining, x.remaining);
    }

    /// P5 — config is table-free and correct: CR1 always enables the peripheral in
    /// master+software-NSS mode, places mode in [1:0] and baud in [5:3], and sets
    /// no reserved/stray bit for any (mode, br_idx).
    #[kani::proof]
    fn p5_cr1_well_formed() {
        let mode: u32 = kani::any();
        let br: u32 = kani::any();
        let v = cr1_value(mode, br);
        // always master, enabled, software-NSS.
        assert!(v & CR1_SPE != 0 && v & CR1_MSTR != 0 && v & CR1_SSM != 0 && v & CR1_SSI != 0);
        // mode occupies exactly CR1[1:0] and equals mode&3.
        assert_eq!(v & 0b11, mode & 0b11);
        // baud occupies exactly BR[5:3] and equals br&7.
        assert_eq!((v >> BR_SHIFT) & 0b111, br & 0b111);
        // no bit outside the defined set is ever set.
        let allowed = CR1_SPE | CR1_MSTR | CR1_SSM | CR1_SSI | (0b111 << BR_SHIFT) | 0b11;
        assert_eq!(v & !allowed, 0);
    }
}
