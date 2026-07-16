//! I-ISO region-programming core — verified MPU program-on-switch.
//!
//! The v0.5.0 isolation keystone: the hardware MPU is the sole isolation
//! root (invariant I-ISO), and THIS core is what programs it on every
//! partition switch. A fault or miscompile inside a partition can then
//! corrupt only WITHIN that partition — the MPU physically denies
//! everything the verified sequence did not explicitly grant.
//!
//! Model: ARMv7-M PMSA (the `mpu` module's hardware constraints — 8
//! regions, power-of-2 sized, base-aligned to size). A static
//! `RegionTable` holds the per-partition region configuration
//! (`MAX_PARTITIONS` × `MAX_REGIONS`, everything scalar/table-free in the
//! thin-seam discipline: no trait objects, no closures, no heap).
//!
//! `program_partition` computes the exact register-write sequence
//! (RNR/RBAR/RASR-shaped scalar triples) for a partition switch, with
//! machine-checked postconditions:
//!
//!   P1 (emitted-matches-table): every emitted region write matches the
//!      table entry exactly (base and encoded size/permissions).
//!   P2 (deny-by-default): every region slot NOT enabled in the
//!      partition's table is emitted DISABLED — unused hardware region
//!      slots are explicitly turned off, never left stale from the
//!      previous partition.
//!   P3 (total): the sequence addresses all `MAX_REGIONS` hardware slots,
//!      exactly one write per slot, no slot skipped.
//!   P4 (disable-first, enable-last): sequence element 0 is the single
//!      MPU_CTRL disable write (`MPU_CTRL := 0`) and the FINAL element is
//!      the single MPU_CTRL enable write — regions are rewritten ONLY
//!      while the MPU is disabled, and the enable bit reaches the
//!      hardware strictly after every region write. There is therefore
//!      no transient window in which a MIXED old/new region map (or a
//!      half-written slot) is live: this is ARM's recommended MPU
//!      reprogramming discipline. While disabled (and until the enable
//!      write) the MPU enforces nothing and privileged code runs under
//!      the default memory map — the switch window is privileged-only
//!      by construction.
//!
//! Platform preconditions (init-time check obligations — see the
//! `mpu_write` seam contract below for the full statement):
//!
//!   * `MPU_TYPE.DREGION == REQUIRED_DREGION` (== 8). On a 16-region
//!     ARMv7-M part (e.g. Cortex-M7 / i.MX RT1176) this sequence would
//!     leave hardware slots 8..=15 STALE, silently defeating P2;
//!     parametrizing the model over DREGION is the named follow-on for
//!     16-region targets.
//!   * DSB/ISB barrier discipline after MPU_CTRL writes (the `mpu_write`
//!     contract, item 1).
//!
//! RASR attribute-field model (deliberate, documented restrictions):
//!
//!   * XN = 0: every granted region is emitted EXECUTABLE — a writable
//!     region is therefore W+X. This is sufficient for the
//!     fault-containment property proven here, but W+X is not acceptable
//!     for the security-containment demo; adding an `executable` bit to
//!     the region model (emitting XN=1 on data regions) is a named
//!     follow-on for the verified table-builder work.
//!   * TEX/C/B/S = 0000: every granted region is strongly-ordered
//!     (uncached, unbuffered, shareable-irrelevant). Correct but slow;
//!     memory-attribute modeling is out of scope for the isolation claim.
//!
//! The mmio EMISSION crosses a trusted extern seam (`mpu_write`, mirroring
//! the executor's `poll_task` seam): the verified core computes the
//! sequence and its proofs; `apply_program`'s LOOP over the sequence is
//! verified (invariant + `decreases`), and only the single register-write
//! call inside it is `#[verifier::external_body]`.
//!
//! Reuses the verified region-validity model from `crate::mpu`
//! (`is_pow2_spec`, `MIN_REGION_SIZE`, and the same well-formedness
//! characterisation `validate_region` enforces at runtime).
use vstd::prelude::*;

// NOTE: `crate::mpu::is_pow2_spec` is referenced fully qualified below —
// it is a spec-only item, and a top-level `use` of it would survive
// `verus-strip` while its definition does not (breaking the plain mirror).
use crate::mpu::MIN_REGION_SIZE;

// ===========================================================================
// Trusted FFI seam — the intersection boundary
// ===========================================================================
//
// `mpu_write` is NOT verified: it performs the actual mmio store of one
// scalar triple into the MPU's RNR/RBAR/RASR registers (or MPU_CTRL for
// the leading disable and trailing enable writes, distinguished by
// `rnr == MPU_CTRL_ID`). It is
// declared outside the verification macro's block below, so it never
// becomes a proof obligation. The only caller is `emit_write`
// (`#[verifier::external_body]`, below), which is itself only reachable
// through the fully verified `apply_program` loop — the same
// minimum-trusted-surface pattern as the executor's `poll_task` seam.
//
// Crate-wide `unsafe_code = "deny"` (Cargo.toml `[lints.rust]`, an ASIL-D
// safety-critical policy) is deliberately overridden here with a single,
// narrowly-scoped `#[allow(unsafe_code)]` — the mmio store is the ONE
// place in this module an FFI call is unavoidable.
#[allow(unsafe_code)]
unsafe extern "C" {
    /// Write one MPU register triple. For `rnr < MAX_REGIONS`: RNR := rnr,
    /// RBAR := rbar, RASR := rasr. For `rnr == MPU_CTRL_ID`: MPU_CTRL :=
    /// rasr (rbar ignored). Implemented by the platform layer.
    ///
    /// # Platform contract (trusted — load-bearing for I-ISO)
    ///
    /// The module's "physically denies" claim holds only on a platform
    /// whose implementation delivers BOTH of the following. They are part
    /// of this extern contract, not optional hardening — an
    /// implementation satisfying only the register-store sentence above
    /// does NOT deliver the guarantee:
    ///
    /// 1. **Barriers (ARMv7-M):** after every `rnr == MPU_CTRL_ID` write
    ///    — both the leading disable and the trailing enable of a program
    ///    sequence — execute `DSB` followed by `ISB` before returning, so
    ///    the MPU reprogramming completes and the new (or disabled)
    ///    region map is in effect for all subsequent instruction fetches
    ///    and data accesses. Without the barriers, accesses issued after
    ///    `apply_program` returns may still be checked against the OLD
    ///    map, and the sequence ordering proven in P4 never reaches the
    ///    hardware.
    /// 2. **Region count (`MPU_TYPE.DREGION == REQUIRED_DREGION`):** at
    ///    init, before the first `switch_to_partition`, the platform MUST
    ///    read `MPU_TYPE.DREGION` and refuse to start if it is not
    ///    exactly `REQUIRED_DREGION` (8). The sequence addresses hardware
    ///    slots `0..MAX_REGIONS` only; on a 16-region ARMv7-M part
    ///    (e.g. Cortex-M7 / i.MX RT1176) slots 8..=15 would be left STALE
    ///    from the previous configuration, silently defeating
    ///    deny-by-default (P2). Parametrizing `MAX_REGIONS` over DREGION
    ///    is the named follow-on for 16-region targets.
    pub(crate) fn mpu_write(rnr: u32, rbar: u32, rasr: u32);
}

verus! {

/// Number of partitions the static table configures.
pub const MAX_PARTITIONS: usize = 4;

/// Hardware region slots per ARMv7-M PMSA MPU (== `crate::mpu::MAX_REGIONS_V7`).
pub const MAX_REGIONS: usize = 8;

/// Total table slots: `MAX_PARTITIONS * MAX_REGIONS`.
pub const TABLE_SLOTS: usize = 32;

/// Length of one program sequence: the single leading MPU_CTRL disable
/// write, all `MAX_REGIONS` region writes, and the single trailing
/// MPU_CTRL enable write.
pub const SEQ_LEN: usize = 10;

/// `MPU_TYPE.DREGION` value this model is proven against. The platform
/// layer MUST check `MPU_TYPE.DREGION == REQUIRED_DREGION` at init,
/// before the first `switch_to_partition` (see the `mpu_write` contract,
/// item 2): on parts with MORE regions the sequence leaves the extra
/// slots stale, defeating deny-by-default. Parametrizing the model over
/// DREGION is the named follow-on for 16-region targets.
pub const REQUIRED_DREGION: u32 = 8;

/// Sentinel register id for the MPU_CTRL enable write (not a region
/// number — region numbers are `0..MAX_REGIONS`).
pub const MPU_CTRL_ID: u32 = 0xFFFF_FFFF;

/// MPU_CTRL value emitted by the switch sequence: ENABLE (bit 0) only.
/// Deliberately NOT setting PRIVDEFENA (bit 2): the background region
/// stays disabled, so anything outside the programmed regions is denied
/// even to privileged code — deny-by-default all the way down.
pub const MPU_CTRL_ENABLE: u32 = 1;

/// MPU_CTRL value emitted as sequence element 0: all bits clear — the
/// MPU is DISABLED before any region is rewritten, so no transient mixed
/// old/new region map is ever enforced (ARM's recommended reprogramming
/// discipline; see P4).
pub const MPU_CTRL_DISABLE: u32 = 0;

/// One scalar register-write triple, RNR/RBAR/RASR-shaped.
#[derive(Clone, Copy)]
pub struct MpuWrite {
    /// Region number (RNR), or `MPU_CTRL_ID` for the trailing enable.
    pub rnr: u32,
    /// Region base address register value (RBAR).
    pub rbar: u32,
    /// Region attribute and size register value (RASR), or the MPU_CTRL
    /// value for the trailing enable write.
    pub rasr: u32,
}

/// The exact register-write sequence for one partition switch: `w[0]` is
/// the single MPU_CTRL disable write, `w[1..=MAX_REGIONS]` are the region
/// writes (hardware slot `r` at position `r + 1`, in slot order), and
/// `w[MAX_REGIONS + 1]` is the single MPU_CTRL enable write.
pub struct ProgramSeq {
    pub w: [MpuWrite; SEQ_LEN],
}

/// Per-partition static region configuration: for each of
/// `MAX_PARTITIONS` partitions × `MAX_REGIONS` hardware slots, one
/// (base, size, enabled, writable) entry, stored flat at index
/// `partition * MAX_REGIONS + region` (see `slot_of`). This is the
/// spec-level model of what the MPU must be programmed to on entry to
/// each partition.
pub struct RegionTable {
    pub base: [u32; TABLE_SLOTS],
    pub size: [u32; TABLE_SLOTS],
    pub enabled: [bool; TABLE_SLOTS],
    pub writable: [bool; TABLE_SLOTS],
}

/// Flat table index of partition `part`'s region slot `r`.
pub open spec fn slot_of(part: u32, r: int) -> int {
    part as int * (MAX_REGIONS as int) + r
}

/// Well-formed region: size is a power of two >= `MIN_REGION_SIZE` (32),
/// base is aligned to size, and the range does not wrap the address
/// space. This is the same characterisation `crate::mpu::validate_region`
/// enforces at runtime (power-of-2 via `is_pow2_spec`, minimum size,
/// alignment, and the U-6 no-overflow bound), restated at spec level.
pub open spec fn region_wf(base: u32, size: u32) -> bool {
    crate::mpu::is_pow2_spec(size)
    && size >= MIN_REGION_SIZE
    && (base as int) % (size as int) == 0
    && base as int + size as int <= u32::MAX as int
}

/// Two regions' address ranges [b1, b1+s1) and [b2, b2+s2) are disjoint.
pub open spec fn regions_disjoint(b1: u32, s1: u32, b2: u32, s2: u32) -> bool {
    b1 as int + s1 as int <= b2 as int || b2 as int + s2 as int <= b1 as int
}

/// Flat indices `i` and `j` belong to the same partition's slot range.
pub open spec fn same_partition(i: int, j: int) -> bool {
    i / (MAX_REGIONS as int) == j / (MAX_REGIONS as int)
}

/// ARMv7-M RASR SIZE field for a power-of-2 region size >= 32:
/// `SIZE = log2(size) - 1` (the hardware region covers `2^(SIZE+1)`
/// bytes). Flat enumeration over the 27 valid sizes — same
/// no-recursive-unfolding style as `is_pow2_spec`. Sizes that are not
/// well-formed map to 0 (never reached under `region_wf`).
pub open spec fn size_field_spec(size: u32) -> u32 {
    if size == 32u32 { 4u32 }
    else if size == 64u32 { 5u32 }
    else if size == 128u32 { 6u32 }
    else if size == 256u32 { 7u32 }
    else if size == 512u32 { 8u32 }
    else if size == 1024u32 { 9u32 }
    else if size == 2048u32 { 10u32 }
    else if size == 4096u32 { 11u32 }
    else if size == 8192u32 { 12u32 }
    else if size == 16384u32 { 13u32 }
    else if size == 32768u32 { 14u32 }
    else if size == 65536u32 { 15u32 }
    else if size == 131072u32 { 16u32 }
    else if size == 262144u32 { 17u32 }
    else if size == 524288u32 { 18u32 }
    else if size == 1048576u32 { 19u32 }
    else if size == 2097152u32 { 20u32 }
    else if size == 4194304u32 { 21u32 }
    else if size == 8388608u32 { 22u32 }
    else if size == 16777216u32 { 23u32 }
    else if size == 33554432u32 { 24u32 }
    else if size == 67108864u32 { 25u32 }
    else if size == 134217728u32 { 26u32 }
    else if size == 268435456u32 { 27u32 }
    else if size == 536870912u32 { 28u32 }
    else if size == 1073741824u32 { 29u32 }
    else if size == 2147483648u32 { 30u32 }
    else { 0u32 }
}

/// ARMv7-M RASR AP field (bits 26:24): 0b011 = privileged+user
/// read-write, 0b110 = privileged+user read-only.
pub open spec fn ap_field_spec(writable: bool) -> u32 {
    if writable { 3u32 } else { 6u32 }
}

/// The RASR value emitted for an ENABLED table entry. Arithmetic encoding
/// of the register fields (equal to the shifted bit layout, stated
/// without bitwise ops so plain linear arithmetic discharges every use):
///   + 1                          — ENABLE, bit 0
///   + size_field * 2             — SIZE field, bits 5:1
///   + ap_field   * 0x0100_0000   — AP field, bits 26:24
/// The ENABLE bit is set (the value is odd); a DISABLED slot is emitted
/// as RASR = 0 (ENABLE bit clear, all fields cleared).
pub open spec fn rasr_enabled_spec(size: u32, writable: bool) -> u32 {
    (1 + 2 * (size_field_spec(size) as int)
        + 0x0100_0000 * (ap_field_spec(writable) as int)) as u32
}

/// Compute the ARMv7-M RASR SIZE field for a well-formed region size.
/// Exec mirror of `size_field_spec`; the trailing branch is unreachable
/// under the `requires` (the power-of-2 enumeration minus the sizes below
/// `MIN_REGION_SIZE` is exactly the 27 cases handled).
pub fn size_field(size: u32) -> (f: u32)
    requires
        crate::mpu::is_pow2_spec(size),
        size >= MIN_REGION_SIZE,
    ensures
        f == size_field_spec(size),
        4 <= f <= 30,
{
    if size == 32 { 4 }
    else if size == 64 { 5 }
    else if size == 128 { 6 }
    else if size == 256 { 7 }
    else if size == 512 { 8 }
    else if size == 1024 { 9 }
    else if size == 2048 { 10 }
    else if size == 4096 { 11 }
    else if size == 8192 { 12 }
    else if size == 16384 { 13 }
    else if size == 32768 { 14 }
    else if size == 65536 { 15 }
    else if size == 131072 { 16 }
    else if size == 262144 { 17 }
    else if size == 524288 { 18 }
    else if size == 1048576 { 19 }
    else if size == 2097152 { 20 }
    else if size == 4194304 { 21 }
    else if size == 8388608 { 22 }
    else if size == 16777216 { 23 }
    else if size == 33554432 { 24 }
    else if size == 67108864 { 25 }
    else if size == 134217728 { 26 }
    else if size == 268435456 { 27 }
    else if size == 536870912 { 28 }
    else if size == 1073741824 { 29 }
    else if size == 2147483648 { 30 }
    else {
        // Unreachable: is_pow2_spec(size) && size >= 32 leaves exactly
        // the 27 sizes handled above.
        proof { assert(false); }
        0
    }
}

/// Encode the RASR value for an ENABLED table entry.
pub fn rasr_for(size: u32, writable: bool) -> (v: u32)
    requires
        crate::mpu::is_pow2_spec(size),
        size >= MIN_REGION_SIZE,
    ensures
        v == rasr_enabled_spec(size, writable),
{
    let f = size_field(size);
    let ap: u32 = if writable { 3 } else { 6 };
    1u32 + 2u32 * f + 0x0100_0000u32 * ap
}

impl RegionTable {
    /// Representation invariant — the anchor every proof rests on:
    /// every enabled region is well-formed (`region_wf`), and enabled
    /// regions of the SAME partition are pairwise disjoint over
    /// [base, base+size).
    pub open spec fn table_inv(&self) -> bool {
        (forall|i: int| 0 <= i < TABLE_SLOTS ==> #[trigger] self.slot_wf(i))
        && (forall|i: int, j: int|
            0 <= i < TABLE_SLOTS && 0 <= j < TABLE_SLOTS && i != j
            && same_partition(i, j)
            && #[trigger] self.slot_enabled(i) && #[trigger] self.slot_enabled(j)
            ==> regions_disjoint(self.base[i], self.size[i], self.base[j], self.size[j]))
    }

    /// Ghost: is table slot `i` enabled?
    pub open spec fn slot_enabled(&self, i: int) -> bool {
        self.enabled[i]
    }

    /// Ghost: slot `i` is well-formed if enabled.
    pub open spec fn slot_wf(&self, i: int) -> bool {
        self.slot_enabled(i) ==> region_wf(self.base[i], self.size[i])
    }

    /// An all-disabled table (the deny-everything baseline). Real
    /// deployments construct their static per-partition configuration as
    /// a constant and discharge `table_inv` at build time.
    pub fn new() -> (t: RegionTable)
        ensures
            t.table_inv(),
            forall|i: int| 0 <= i < TABLE_SLOTS ==> !(#[trigger] t.slot_enabled(i)),
    {
        RegionTable {
            base: [0u32; TABLE_SLOTS],
            size: [0u32; TABLE_SLOTS],
            enabled: [false; TABLE_SLOTS],
            writable: [false; TABLE_SLOTS],
        }
    }

    /// Compute the exact register-write sequence for switching the MPU to
    /// partition `part` — the verified heart of I-ISO. See the module
    /// header for P1–P4.
    pub fn program_partition(&self, part: u32) -> (out: ProgramSeq)
        requires
            self.table_inv(),
            part < MAX_PARTITIONS as u32,
        ensures
            // P4a (disable-first): sequence element 0 is the single
            // MPU_CTRL disable write — the MPU is off before any region
            // is rewritten, so no transient mixed old/new map is ever
            // enforced.
            out.w[0].rnr == MPU_CTRL_ID,
            out.w[0].rbar == 0u32,
            out.w[0].rasr == MPU_CTRL_DISABLE,
            // P3 (total): all MAX_REGIONS hardware slots addressed,
            // exactly one write per slot — hardware slot p-1 at sequence
            // position p — no slot skipped, none written twice, none
            // stale.
            forall|p: int| 1 <= p <= MAX_REGIONS ==>
                (#[trigger] out.w[p]).rnr == (p - 1) as u32,
            // P1 (emitted-matches-table): every enabled slot's write
            // carries exactly the table's base and encoded size/attrs.
            forall|p: int| 1 <= p <= MAX_REGIONS && self.enabled[slot_of(part, p - 1)] ==>
                (#[trigger] out.w[p]).rbar == self.base[slot_of(part, p - 1)]
                && out.w[p].rasr == rasr_enabled_spec(
                    self.size[slot_of(part, p - 1)],
                    self.writable[slot_of(part, p - 1)],
                ),
            // P2 (deny-by-default): every slot NOT enabled in the
            // partition's table is emitted DISABLED (RASR == 0, ENABLE
            // bit clear) — never left stale.
            forall|p: int| 1 <= p <= MAX_REGIONS && !self.enabled[slot_of(part, p - 1)] ==>
                (#[trigger] out.w[p]).rbar == 0u32 && out.w[p].rasr == 0u32,
            // P4b (enable-last): the single MPU_CTRL enable write is the
            // LAST element of the sequence — all region programming
            // precedes the enable bit, and (with P4a) happens only while
            // the MPU is disabled.
            out.w[MAX_REGIONS as int + 1].rnr == MPU_CTRL_ID,
            out.w[MAX_REGIONS as int + 1].rbar == 0u32,
            out.w[MAX_REGIONS as int + 1].rasr == MPU_CTRL_ENABLE,
    {
        let mut out = ProgramSeq {
            w: [MpuWrite { rnr: 0, rbar: 0, rasr: 0 }; SEQ_LEN],
        };
        // P4a — element 0: disable the MPU before any region rewrite.
        out.w[0] = MpuWrite { rnr: MPU_CTRL_ID, rbar: 0, rasr: MPU_CTRL_DISABLE };
        let mut r: usize = 0;
        while r < MAX_REGIONS
            invariant
                // Loop bodies are verified against the invariant list
                // alone — restate the function's requires.
                self.table_inv(),
                part < MAX_PARTITIONS as u32,
                0 <= r <= MAX_REGIONS,
                out.w[0].rnr == MPU_CTRL_ID,
                out.w[0].rbar == 0u32,
                out.w[0].rasr == MPU_CTRL_DISABLE,
                forall|p: int| 1 <= p <= r ==>
                    (#[trigger] out.w[p]).rnr == (p - 1) as u32,
                forall|p: int| 1 <= p <= r && self.enabled[slot_of(part, p - 1)] ==>
                    (#[trigger] out.w[p]).rbar == self.base[slot_of(part, p - 1)]
                    && out.w[p].rasr == rasr_enabled_spec(
                        self.size[slot_of(part, p - 1)],
                        self.writable[slot_of(part, p - 1)],
                    ),
                forall|p: int| 1 <= p <= r && !self.enabled[slot_of(part, p - 1)] ==>
                    (#[trigger] out.w[p]).rbar == 0u32 && out.w[p].rasr == 0u32,
            decreases MAX_REGIONS - r,
        {
            let i = (part as usize) * MAX_REGIONS + r;
            if self.enabled[i] {
                proof {
                    // Instantiate table_inv's per-slot forall at i to
                    // obtain region_wf for rasr_for's requires.
                    assert(self.slot_wf(i as int));
                }
                let rasr = rasr_for(self.size[i], self.writable[i]);
                out.w[r + 1] = MpuWrite { rnr: r as u32, rbar: self.base[i], rasr };
            } else {
                out.w[r + 1] = MpuWrite { rnr: r as u32, rbar: 0, rasr: 0 };
            }
            r += 1;
        }
        out.w[MAX_REGIONS + 1] = MpuWrite { rnr: MPU_CTRL_ID, rbar: 0, rasr: MPU_CTRL_ENABLE };
        out
    }

    /// Program the MPU for partition `part`: compute the verified
    /// sequence, then emit it through the trusted seam — the one call a
    /// partition switch makes. The computation and the emission loop are
    /// verified; only the single register store is trusted.
    pub fn switch_to_partition(&self, part: u32)
        requires
            self.table_inv(),
            part < MAX_PARTITIONS as u32,
    {
        let seq = self.program_partition(part);
        apply_program(&seq);
    }
}

/// The trusted FFI seam, wrapped to the minimum trusted surface: hand ONE
/// scalar triple to the platform's register-store routine.
/// `#[verifier::external_body]` means the body is not checked; it carries
/// no `ensures` at all, so no proof anywhere rests on what the store did
/// — the trusted annotation carries as little weight as possible.
#[verifier::external_body]
#[allow(unsafe_code)] // see the trusted-seam note at the top of this file
fn emit_write(w: &MpuWrite) {
    unsafe { mpu_write(w.rnr, w.rbar, w.rasr) };
}

/// Emit a computed program sequence, in sequence order. The loop is
/// verified (invariant + `decreases` — it visits every element exactly
/// once and terminates); only `emit_write`'s single register store is
/// external. Because `ProgramSeq` places the MPU_CTRL disable at index 0
/// and the MPU_CTRL enable at the final index (P4), in-order emission
/// guarantees the MPU is disabled before any region write reaches the
/// hardware and re-enabled only after all of them (given the `mpu_write`
/// barrier contract).
pub fn apply_program(seq: &ProgramSeq) {
    let mut i: usize = 0;
    while i < SEQ_LEN
        invariant
            0 <= i <= SEQ_LEN,
        decreases SEQ_LEN - i,
    {
        emit_write(&seq.w[i]);
        i += 1;
    }
}

/// Kani cross-check: `program_partition` (Verus-proven above via SMT/Z3)
/// under Kani's bounded model checker (SAT-based CBMC — an independent
/// engine) against independently-computed expectations. As with the
/// executor's harnesses, these run the SAME shipped executable code path
/// (post-`verus-strip`, the ghost clauses are gone and the body is plain
/// executable Rust — Kani calls that exact function, no hand-copied
/// mirror). `table_inv` is spec-only (stripped), so the harnesses assume
/// its exec-checkable equivalent: `crate::mpu::validate_region` per
/// enabled slot (power-of-2 / minimum-size / alignment / no-overflow —
/// the same characterisation `region_wf` states in spec) plus explicit
/// pairwise range-disjointness.
#[cfg(kani)]
mod iso_kani {
    use super::*;
    use crate::mpu::validate_region;

    /// An arbitrary table + partition choice satisfying the exec
    /// equivalent of `table_inv` on the chosen partition's slots (the
    /// only slots `program_partition` reads).
    fn arbitrary_table_and_partition() -> (RegionTable, u32) {
        let base: [u32; TABLE_SLOTS] = kani::any();
        let size: [u32; TABLE_SLOTS] = kani::any();
        let enabled: [bool; TABLE_SLOTS] = kani::any();
        let writable: [bool; TABLE_SLOTS] = kani::any();
        let t = RegionTable { base, size, enabled, writable };
        let part: u32 = kani::any();
        kani::assume(part < MAX_PARTITIONS as u32);
        let p = part as usize;
        // table_inv, exec form, on partition `part`'s slots:
        // every enabled slot well-formed...
        for r in 0..MAX_REGIONS {
            let i = p * MAX_REGIONS + r;
            if t.enabled[i] {
                kani::assume(validate_region(t.base[i], t.size[i]));
            }
        }
        // ...and enabled slots pairwise disjoint over [base, base+size).
        for r1 in 0..MAX_REGIONS {
            for r2 in 0..MAX_REGIONS {
                if r1 != r2 {
                    let i1 = p * MAX_REGIONS + r1;
                    let i2 = p * MAX_REGIONS + r2;
                    if t.enabled[i1] && t.enabled[i2] {
                        let e1 = t.base[i1] as u64 + t.size[i1] as u64;
                        let e2 = t.base[i2] as u64 + t.size[i2] as u64;
                        kani::assume(e1 <= t.base[i2] as u64 || e2 <= t.base[i1] as u64);
                    }
                }
            }
        }
        (t, part)
    }

    /// k1 — deny-by-default: every slot not enabled in the table emits
    /// RASR with the ENABLE bit (bit 0) clear (in fact fully zeroed).
    #[kani::proof]
    #[kani::unwind(33)]
    fn iso_deny_by_default() {
        let (t, part) = arbitrary_table_and_partition();
        let seq = t.program_partition(part);
        for r in 0..MAX_REGIONS {
            let i = part as usize * MAX_REGIONS + r;
            if !t.enabled[i] {
                assert!(seq.w[r + 1].rasr & 1 == 0);
                assert!(seq.w[r + 1].rasr == 0);
                assert!(seq.w[r + 1].rbar == 0);
            }
        }
    }

    /// k2 — emitted-matches-table: every enabled slot's write carries the
    /// table's base, the ENABLE bit set, the SIZE field independently
    /// recomputed via `trailing_zeros` (log2 of a power of two), and the
    /// AP field matching `writable`.
    #[kani::proof]
    #[kani::unwind(33)]
    fn iso_emitted_matches_table() {
        let (t, part) = arbitrary_table_and_partition();
        let seq = t.program_partition(part);
        for r in 0..MAX_REGIONS {
            let i = part as usize * MAX_REGIONS + r;
            if t.enabled[i] {
                assert!(seq.w[r + 1].rbar == t.base[i]);
                // ENABLE bit set
                assert!(seq.w[r + 1].rasr & 1 == 1);
                // SIZE field (bits 5:1) == log2(size) - 1, recomputed
                // independently of the shipped encoder's lookup chain.
                let expect_field = t.size[i].trailing_zeros() - 1;
                assert!((seq.w[r + 1].rasr >> 1) & 0x1F == expect_field);
                // AP field (bits 26:24): RW = 0b011, RO = 0b110.
                let expect_ap = if t.writable[i] { 3u32 } else { 6u32 };
                assert!((seq.w[r + 1].rasr >> 24) & 0x7 == expect_ap);
            }
        }
    }

    /// k3 — disjointness preserved: no two ENABLED emitted regions
    /// overlap, with each region's extent decoded back OUT of the emitted
    /// RASR SIZE field (not read from the table).
    #[kani::proof]
    #[kani::unwind(33)]
    fn iso_emissions_disjoint() {
        let (t, part) = arbitrary_table_and_partition();
        let seq = t.program_partition(part);
        for r1 in 0..MAX_REGIONS {
            for r2 in 0..MAX_REGIONS {
                let p1 = r1 + 1;
                let p2 = r2 + 1;
                if r1 != r2 && seq.w[p1].rasr & 1 == 1 && seq.w[p2].rasr & 1 == 1 {
                    let s1 = 1u64 << (((seq.w[p1].rasr >> 1) & 0x1F) + 1);
                    let s2 = 1u64 << (((seq.w[p2].rasr >> 1) & 0x1F) + 1);
                    let b1 = seq.w[p1].rbar as u64;
                    let b2 = seq.w[p2].rbar as u64;
                    assert!(b1 + s1 <= b2 || b2 + s2 <= b1);
                }
            }
        }
    }

    /// k4 — sequence-total + ordered: the MPU_CTRL DISABLE write is the
    /// FIRST element (regions are rewritten only while the MPU is off),
    /// all 8 region slots are emitted exactly once (slot r at position
    /// r + 1, so no slot skipped and no duplicates), and the single
    /// MPU_CTRL enable write is the final element.
    #[kani::proof]
    #[kani::unwind(33)]
    fn iso_sequence_total_and_ordered() {
        let (t, part) = arbitrary_table_and_partition();
        let seq = t.program_partition(part);
        // Disable-first: element 0 turns the MPU off (ENABLE bit clear).
        assert!(seq.w[0].rnr == MPU_CTRL_ID);
        assert!(seq.w[0].rasr == MPU_CTRL_DISABLE);
        assert!(seq.w[0].rasr & 1 == 0);
        for r in 0..MAX_REGIONS {
            assert!(seq.w[r + 1].rnr == r as u32);
            assert!(seq.w[r + 1].rnr != MPU_CTRL_ID);
        }
        assert!(seq.w[MAX_REGIONS + 1].rnr == MPU_CTRL_ID);
        assert!(seq.w[MAX_REGIONS + 1].rasr == MPU_CTRL_ENABLE);
    }
}

} // verus!
