//! gust-dma-own — DMA as Component-Model `own<buffer>` handoff, the verified core.
//!
//! gale#124. DMA is the one path that violates wasm's linear-memory assumption:
//! an external agent (the DMA engine) mutates memory the engine treats as private.
//! We model a transfer as an **ownership round-trip** — `own<buffer>` moves from
//! wasm to the DMA agent and back on completion — so that:
//!   * wasm is provably hands-off the buffer for the transfer window (safety), and
//!   * synth learns exactly WHEN the region is externally-mutable vs private
//!     (the codegen signal: optimize freely while wasm-owned, back off while
//!     DMA-owned). See the shared-segment region marking (synth#390 / loom#226).
//!
//! This file is the **verifiable core**: a total, Kani-proven transfer state
//! machine (single round-trip AND streaming/circular per-chunk), with the
//! cache/barrier op emitted *by construction* at every ownership handoff — so it
//! is structurally impossible to transfer without the paired coherency op. The
//! trusted native surface stays the irreducible atoms (MMIO descriptor poke, IRQ
//! shim); this state machine — the part that decides who may touch the buffer —
//! is verified wasm, dissolved to native like every other gust primitive.
//!
//! Mirrors the uart-thin idiom: `#![no_std]` for the wasm32 dissolve target;
//! under `cargo kani` we build for the host so the model checker exercises the
//! pure logic. The exported `#[no_mangle]` primitives are the dissolve ABI; the
//! TCB bridge implements the descriptor-program + IRQ-shim imports.
#![cfg_attr(not(kani), no_std)]

#[cfg(not(kani))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

// ── gust:hal DMA capability imports — become import-call relocations in the
//    dissolved object, resolved at link by the TCB bridge. The counterparty is
//    silicon (a non-wasm trusted agent), so these are the irreducible atoms:
//    program the descriptor (a poke), and the barrier/cache op at the handoff.
extern "C" {
    /// Program the DMA descriptor for `channel`: src/dst/len already in the
    /// shared segment; this is the trusted register poke that arms the engine.
    fn dma_program(channel: u32, len: u32);
    /// The cache/barrier op that MUST accompany a handoff. `op` is a `Barrier`
    /// discriminant. No-op on M3 (no cache); real clean/invalidate + DSB/DMB on
    /// M7/A-class. Kept behind the seam so the abstraction holds everywhere.
    fn dma_barrier(op: u32);
    /// irq.poll(channel): nonzero if the completion IRQ fired since last poll
    /// (and clears it). The existing split-phase companion — wakes the kiln
    /// waitable that resolves `future<own<buffer>>`.
    fn dma_irq_poll(channel: u32) -> u32;
}

// ─────────────────────────── the verifiable core ───────────────────────────

/// Who currently owns a DMA buffer (or one chunk of a circular buffer).
/// Exactly one owner at all times — never both, never neither (proven).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Owner {
    /// wasm holds `own<buffer>`; may read/write; synth may treat as **private**.
    Wasm,
    /// the DMA agent owns it; wasm provably cannot touch; synth must treat the
    /// region as **externally-mutable** (no caching loads across this window).
    Dma,
}

/// The cache/barrier op emitted *by construction* at an ownership handoff.
/// The handoff points ARE the coherency events; pairing them with the transition
/// (not by convention) makes it impossible to forget the barrier.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Barrier {
    /// Before a DMA *write into memory the CPU cached*: clean (flush) + DSB.
    /// Emitted on Wasm → Dma.
    CleanDsb,
    /// After a DMA *write*, before the CPU reads: invalidate + DMB.
    /// Emitted on Dma → Wasm (completion and abort).
    InvalidateDmb,
}

impl Barrier {
    /// Stable discriminant for the `dma_barrier` seam ABI (scalar, no linmem).
    pub const fn code(self) -> u32 {
        match self {
            Barrier::CleanDsb => 0,
            Barrier::InvalidateDmb => 1,
        }
    }
}

/// Why a transition was rejected. A faulted transfer never leaves the buffer
/// ownerless — `abort` always returns it to a defined owner.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// `start` when the buffer is already DMA-owned (double-arm).
    NotWasmOwned,
    /// `complete` when the buffer is not DMA-owned (completion without a start).
    NotDmaOwned,
}

/// A single-buffer transfer's ownership state. (The circular case composes N of
/// these as a ring, below.) `Owner` IS the state — there is no separate "in
/// flight" limbo, which is what keeps "no ownerless state" trivially true.
pub type XferState = Owner;

/// Access predicate: wasm may touch the buffer **iff** it is Wasm-owned. This is
/// the safety property the whole design exists to guarantee. The dissolved API
/// gates every buffer read/write on this.
#[inline]
pub const fn wasm_may_access(s: XferState) -> bool {
    matches!(s, Owner::Wasm)
}

/// Start a transfer: consume `own<buffer>` (Wasm → Dma), emitting `CleanDsb`.
/// The barrier is returned WITH the new state — the caller cannot advance
/// ownership without also receiving (and the bridge, emitting) the coherency op.
#[inline]
pub const fn start(s: XferState) -> Result<(XferState, Barrier), Fault> {
    match s {
        Owner::Wasm => Ok((Owner::Dma, Barrier::CleanDsb)),
        Owner::Dma => Err(Fault::NotWasmOwned),
    }
}

/// Complete a transfer: return `own<buffer>` (Dma → Wasm), emitting
/// `InvalidateDmb`. Driven by the completion IRQ via the kiln waitable.
#[inline]
pub const fn complete(s: XferState) -> Result<(XferState, Barrier), Fault> {
    match s {
        Owner::Dma => Ok((Owner::Wasm, Barrier::InvalidateDmb)),
        Owner::Wasm => Err(Fault::NotDmaOwned),
    }
}

/// Abort / channel teardown: return the buffer to a well-defined owner from ANY
/// state — never ownerless. A faulted or torn-down transfer lands Wasm-owned
/// (with `InvalidateDmb`, the coherency-safe default in case the engine wrote
/// partial data). Total: defined for every state.
#[inline]
pub const fn abort(_s: XferState) -> (XferState, Barrier) {
    (Owner::Wasm, Barrier::InvalidateDmb)
}

// ───────────────────── streaming / circular (v1 scope) ─────────────────────
//
// A circular / double-buffered transfer is a ring of N chunks, each with its own
// `Owner`. The producer hands the next wasm-owned chunk to DMA (advancing the
// arm cursor); completion returns the oldest dma-owned chunk to wasm (advancing
// the reap cursor). Per-chunk `own` handoff = the `stream<chunk>` shape in WIT.
// Each chunk is owned by exactly one side at all times (proven), so streaming is
// the single-buffer property applied N-wise, not a new trust story.

/// Fixed ring size for the verifiable model (a real channel picks its own N; the
/// proof is parametric in position via `kani::any` indices). Small for Kani
/// tractability; the invariants are size-independent.
pub const RING: usize = 4;

/// A circular DMA transfer: `owner[i]` is the owner of chunk `i`. `armed` chunks
/// (Dma) form a contiguous window `[reap, arm)` mod RING; the rest are Wasm.
#[derive(Clone, Copy)]
pub struct Ring {
    owner: [Owner; RING],
}

impl Ring {
    /// Fresh ring: every chunk wasm-owned (nothing in flight).
    pub const fn new() -> Self {
        Ring { owner: [Owner::Wasm; RING] }
    }

    /// wasm may access chunk `i` iff that chunk is wasm-owned.
    #[inline]
    pub fn may_access(&self, i: usize) -> bool {
        i < RING && matches!(self.owner[i], Owner::Wasm)
    }

    /// Arm chunk `i` (Wasm → Dma) — hand one `own<chunk>` to the engine.
    /// Emits `CleanDsb`. Rejects if the chunk is not wasm-owned (already armed).
    #[inline]
    pub fn arm(&mut self, i: usize) -> Result<Barrier, Fault> {
        if i >= RING {
            return Err(Fault::NotWasmOwned);
        }
        match self.owner[i] {
            Owner::Wasm => {
                self.owner[i] = Owner::Dma;
                Ok(Barrier::CleanDsb)
            }
            Owner::Dma => Err(Fault::NotWasmOwned),
        }
    }

    /// Reap chunk `i` (Dma → Wasm) on its completion IRQ. Emits `InvalidateDmb`.
    #[inline]
    pub fn reap(&mut self, i: usize) -> Result<Barrier, Fault> {
        if i >= RING {
            return Err(Fault::NotDmaOwned);
        }
        match self.owner[i] {
            Owner::Dma => {
                self.owner[i] = Owner::Wasm;
                Ok(Barrier::InvalidateDmb)
            }
            Owner::Wasm => Err(Fault::NotDmaOwned),
        }
    }

    /// Abort the whole ring: every chunk returns to wasm (never ownerless).
    #[inline]
    pub fn abort_all(&mut self) -> Barrier {
        let mut k = 0;
        while k < RING {
            self.owner[k] = Owner::Wasm;
            k += 1;
        }
        Barrier::InvalidateDmb
    }
}

impl Default for Ring {
    fn default() -> Self {
        Self::new()
    }
}

// ───────────────────────────── dissolve ABI ─────────────────────────────────
//
// The exported primitives the composed image calls (scalar in/out, no linmem
// data → no r11 trampoline, like uart-thin). State lives in the caller (the
// composed app / kiln task); these are pure transitions over a `u32`-encoded
// owner, each poking the trusted descriptor/barrier seam at the handoff.

/// Encode owner as the scalar the dissolve ABI passes (0 = Wasm, 1 = Dma).
#[inline]
const fn enc(o: Owner) -> u32 {
    match o {
        Owner::Wasm => 0,
        Owner::Dma => 1,
    }
}
#[inline]
const fn dec(s: u32) -> Owner {
    if s == 0 {
        Owner::Wasm
    } else {
        Owner::Dma
    }
}

/// Sentinel returned by the transition ABI on a rejected transition (fault) —
/// keeps the ABI scalar (no Result across the boundary), same idiom as
/// uart-thin's `RX_NONE`.
pub const XFER_FAULT: u32 = 0xFFFF_FFFF;

/// Start a single-buffer transfer on `channel` of `len` bytes. Returns the new
/// owner-state (`enc`) or `XFER_FAULT`. Emits the clean+DSB barrier and programs
/// the descriptor — both through the trusted seam — before handing ownership off.
///
/// # Safety
/// Calls the trusted `dma_barrier`/`dma_program` seam (MMIO/descriptor pokes).
#[no_mangle]
pub unsafe extern "C" fn dma_start(state: u32, channel: u32, len: u32) -> u32 {
    match start(dec(state)) {
        Ok((next, bar)) => {
            dma_barrier(bar.code()); // clean+DSB BEFORE the engine reads memory
            dma_program(channel, len); // arm the descriptor (trusted poke)
            enc(next)
        }
        Err(_) => XFER_FAULT,
    }
}

/// Poll the completion IRQ for `channel`; if fired, complete the transfer
/// (Dma → Wasm) emitting invalidate+DMB and returning the re-owned state. If not
/// fired, returns the input `state` unchanged (the kiln poll yields). Fault (a
/// completion with no outstanding transfer) returns `XFER_FAULT`.
///
/// # Safety
/// Calls the trusted `dma_irq_poll`/`dma_barrier` seam.
#[no_mangle]
pub unsafe extern "C" fn dma_poll_complete(state: u32, channel: u32) -> u32 {
    if dma_irq_poll(channel) == 0 {
        return state; // not yet — yield to the scheduler
    }
    match complete(dec(state)) {
        Ok((next, bar)) => {
            dma_barrier(bar.code()); // invalidate+DMB AFTER the engine wrote
            enc(next)
        }
        Err(_) => XFER_FAULT,
    }
}

/// Abort the transfer on `channel`: return ownership to wasm from any state,
/// emitting the coherency-safe barrier. Never faults, never ownerless.
///
/// # Safety
/// Calls the trusted `dma_barrier` seam.
#[no_mangle]
pub unsafe extern "C" fn dma_abort(state: u32, _channel: u32) -> u32 {
    let (next, bar) = abort(dec(state));
    dma_barrier(bar.code());
    enc(next)
}

// ─────────────────────────────── Kani proofs ────────────────────────────────
//
// The safety story, machine-checked over the full input space. Run: `cargo kani`.
#[cfg(kani)]
mod kani_proofs {
    use super::*;

    fn any_owner() -> Owner {
        if kani::any() {
            Owner::Wasm
        } else {
            Owner::Dma
        }
    }

    /// P1 — exclusive access: wasm may touch the buffer IFF it is wasm-owned.
    /// While DMA-owned, access is provably denied (the core safety property).
    #[kani::proof]
    fn p1_access_iff_wasm_owned() {
        let s = any_owner();
        assert_eq!(wasm_may_access(s), s == Owner::Wasm);
        if s == Owner::Dma {
            assert!(!wasm_may_access(s)); // hands-off during the transfer window
        }
    }

    /// P2 — barrier pairing by construction: every ownership handoff carries the
    /// correct coherency op, on EVERY path. No transition transfers without it.
    #[kani::proof]
    fn p2_barrier_pairing() {
        let s = any_owner();
        // start: only from Wasm, and always clean+DSB.
        match start(s) {
            Ok((next, bar)) => {
                assert_eq!(s, Owner::Wasm);
                assert_eq!(next, Owner::Dma);
                assert_eq!(bar, Barrier::CleanDsb);
            }
            Err(f) => assert!(s == Owner::Dma && f == Fault::NotWasmOwned),
        }
        // complete: only from Dma, and always invalidate+DMB.
        match complete(s) {
            Ok((next, bar)) => {
                assert_eq!(s, Owner::Dma);
                assert_eq!(next, Owner::Wasm);
                assert_eq!(bar, Barrier::InvalidateDmb);
            }
            Err(f) => assert!(s == Owner::Wasm && f == Fault::NotDmaOwned),
        }
    }

    /// P3 — no ownerless state: abort from ANY state returns the buffer to a
    /// defined owner (wasm), with a barrier. Total; never a limbo/gap.
    #[kani::proof]
    fn p3_abort_never_ownerless() {
        let s = any_owner();
        let (next, bar) = abort(s);
        assert_eq!(next, Owner::Wasm);
        assert_eq!(bar, Barrier::InvalidateDmb);
    }

    /// P4 — round-trip integrity: start then complete returns to the original
    /// wasm-owned state; a doubled start or an unpaired complete is a fault, not
    /// a silent state corruption.
    #[kani::proof]
    fn p4_round_trip() {
        // clean round-trip from Wasm
        let (mid, _) = start(Owner::Wasm).unwrap();
        assert_eq!(mid, Owner::Dma);
        assert!(!wasm_may_access(mid)); // hands-off in the middle
        let (end, _) = complete(mid).unwrap();
        assert_eq!(end, Owner::Wasm);
        assert!(wasm_may_access(end)); // re-owned
        // double-start and unpaired-complete are rejected
        assert!(start(Owner::Dma).is_err());
        assert!(complete(Owner::Wasm).is_err());
    }

    /// P5 — streaming per-chunk exclusivity: over an arbitrary ring and an
    /// arbitrary chunk index, arm/reap flip exactly that chunk's owner and never
    /// leave a chunk both- or neither-owned; access follows ownership per chunk.
    #[kani::proof]
    fn p5_ring_per_chunk_exclusive() {
        let mut r = Ring::new();
        // arbitrary starting ownership per chunk
        let mut k = 0;
        while k < RING {
            r.owner[k] = any_owner();
            k += 1;
        }
        let i: usize = kani::any();
        kani::assume(i < RING);
        let before = r.owner[i];

        // access predicate tracks ownership exactly, for the chosen chunk.
        assert_eq!(r.may_access(i), before == Owner::Wasm);

        // arm flips only chunk i (Wasm→Dma) or faults if already Dma.
        let snapshot = r.owner;
        match r.arm(i) {
            Ok(bar) => {
                assert_eq!(before, Owner::Wasm);
                assert_eq!(r.owner[i], Owner::Dma);
                assert_eq!(bar, Barrier::CleanDsb);
                assert!(!r.may_access(i)); // now DMA-owned → no access
            }
            Err(_) => assert_eq!(before, Owner::Dma),
        }
        // no OTHER chunk changed owner.
        let mut j = 0;
        while j < RING {
            if j != i {
                assert_eq!(r.owner[j], snapshot[j]);
            }
            j += 1;
        }
    }

    /// P6 — ring reap + abort: reap returns exactly one chunk to wasm with the
    /// read barrier; abort_all returns every chunk to wasm (never ownerless).
    #[kani::proof]
    fn p6_ring_reap_and_abort() {
        let mut r = Ring::new();
        let i: usize = kani::any();
        kani::assume(i < RING);
        r.owner[i] = Owner::Dma; // one chunk in flight
        let bar = r.reap(i).unwrap();
        assert_eq!(r.owner[i], Owner::Wasm);
        assert_eq!(bar, Barrier::InvalidateDmb);

        // set an arbitrary ring, abort, prove all wasm-owned.
        let mut k = 0;
        while k < RING {
            r.owner[k] = any_owner();
            k += 1;
        }
        let b = r.abort_all();
        assert_eq!(b, Barrier::InvalidateDmb);
        let mut j = 0;
        while j < RING {
            assert_eq!(r.owner[j], Owner::Wasm);
            assert!(r.may_access(j));
            j += 1;
        }
    }
}
