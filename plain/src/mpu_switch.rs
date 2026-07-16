
// NOTE: `crate::mpu::is_pow2_spec` is referenced fully qualified below —
// it is a spec-only item, and a top-level `use` of it would survive
// `verus-strip` while its definition does not (breaking the plain mirror).

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
//!   P4 (ordered): the single MPU_CTRL enable write is the LAST element
//!      of the sequence — all region programming precedes any enable bit.
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
use crate::mpu::MIN_REGION_SIZE;
#[allow(unsafe_code)]
unsafe extern "C" {
    /// Write one MPU register triple. For `rnr < MAX_REGIONS`: RNR := rnr,
    /// RBAR := rbar, RASR := rasr. For `rnr == MPU_CTRL_ID`: MPU_CTRL :=
    /// rasr (rbar ignored). Implemented by the platform layer.
    pub fn mpu_write(rnr: u32, rbar: u32, rasr: u32);
}
/// Number of partitions the static table configures.
pub const MAX_PARTITIONS: usize = 4;
/// Hardware region slots per ARMv7-M PMSA MPU (== `crate::mpu::MAX_REGIONS_V7`).
pub const MAX_REGIONS: usize = 8;
/// Total table slots: `MAX_PARTITIONS * MAX_REGIONS`.
pub const TABLE_SLOTS: usize = 32;
/// Length of one program sequence: all `MAX_REGIONS` region writes plus
/// the single trailing MPU_CTRL enable write.
pub const SEQ_LEN: usize = 9;
/// Sentinel register id for the MPU_CTRL enable write (not a region
/// number — region numbers are `0..MAX_REGIONS`).
pub const MPU_CTRL_ID: u32 = 0xFFFF_FFFF;
/// MPU_CTRL value emitted by the switch sequence: ENABLE (bit 0) only.
/// Deliberately NOT setting PRIVDEFENA (bit 2): the background region
/// stays disabled, so anything outside the programmed regions is denied
/// even to privileged code — deny-by-default all the way down.
pub const MPU_CTRL_ENABLE: u32 = 1;
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
/// The exact register-write sequence for one partition switch:
/// `w[0..MAX_REGIONS]` are the region writes (one per hardware slot, in
/// slot order), `w[MAX_REGIONS]` is the single MPU_CTRL enable write.
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
/// Compute the ARMv7-M RASR SIZE field for a well-formed region size.
/// Exec mirror of `size_field_spec`; the trailing branch is unreachable
/// under the `requires` (the power-of-2 enumeration minus the sizes below
/// `MIN_REGION_SIZE` is exactly the 27 cases handled).
pub fn size_field(size: u32) -> u32 {
    if size == 32 {
        4
    } else if size == 64 {
        5
    } else if size == 128 {
        6
    } else if size == 256 {
        7
    } else if size == 512 {
        8
    } else if size == 1024 {
        9
    } else if size == 2048 {
        10
    } else if size == 4096 {
        11
    } else if size == 8192 {
        12
    } else if size == 16384 {
        13
    } else if size == 32768 {
        14
    } else if size == 65536 {
        15
    } else if size == 131072 {
        16
    } else if size == 262144 {
        17
    } else if size == 524288 {
        18
    } else if size == 1048576 {
        19
    } else if size == 2097152 {
        20
    } else if size == 4194304 {
        21
    } else if size == 8388608 {
        22
    } else if size == 16777216 {
        23
    } else if size == 33554432 {
        24
    } else if size == 67108864 {
        25
    } else if size == 134217728 {
        26
    } else if size == 268435456 {
        27
    } else if size == 536870912 {
        28
    } else if size == 1073741824 {
        29
    } else if size == 2147483648 {
        30
    } else {
        0
    }
}
/// Encode the RASR value for an ENABLED table entry.
pub fn rasr_for(size: u32, writable: bool) -> u32 {
    let f = size_field(size);
    let ap: u32 = if writable { 3 } else { 6 };
    1u32 + 2u32 * f + 0x0100_0000u32 * ap
}
impl RegionTable {
    /// An all-disabled table (the deny-everything baseline). Real
    /// deployments construct their static per-partition configuration as
    /// a constant and discharge `table_inv` at build time.
    pub fn new() -> RegionTable {
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
    pub fn program_partition(&self, part: u32) -> ProgramSeq {
        let mut out = ProgramSeq {
            w: [MpuWrite {
                rnr: 0,
                rbar: 0,
                rasr: 0,
            }; SEQ_LEN],
        };
        let mut r: usize = 0;
        while r < MAX_REGIONS {
            let i = (part as usize) * MAX_REGIONS + r;
            if self.enabled[i] {
                let rasr = rasr_for(self.size[i], self.writable[i]);
                out.w[r] = MpuWrite {
                    rnr: r as u32,
                    rbar: self.base[i],
                    rasr,
                };
            } else {
                out.w[r] = MpuWrite {
                    rnr: r as u32,
                    rbar: 0,
                    rasr: 0,
                };
            }
            r += 1;
        }
        out.w[MAX_REGIONS] = MpuWrite {
            rnr: MPU_CTRL_ID,
            rbar: 0,
            rasr: MPU_CTRL_ENABLE,
        };
        out
    }
    /// Program the MPU for partition `part`: compute the verified
    /// sequence, then emit it through the trusted seam — the one call a
    /// partition switch makes. The computation and the emission loop are
    /// verified; only the single register store is trusted.
    pub fn switch_to_partition(&self, part: u32) {
        let seq = self.program_partition(part);
        apply_program(&seq);
    }
}
/// The trusted FFI seam, wrapped to the minimum trusted surface: hand ONE
/// scalar triple to the platform's register-store routine.
/// `#[verifier::external_body]` means the body is not checked; it carries
/// no `ensures` at all, so no proof anywhere rests on what the store did
/// — the trusted annotation carries as little weight as possible.
#[allow(unsafe_code)]
fn emit_write(w: &MpuWrite) {
    unsafe { mpu_write(w.rnr, w.rbar, w.rasr) };
}
/// Emit a computed program sequence, in sequence order. The loop is
/// verified (invariant + `decreases` — it visits every element exactly
/// once and terminates); only `emit_write`'s single register store is
/// external. Because `ProgramSeq` places the MPU_CTRL enable at the final
/// index (P4), in-order emission guarantees all region programming
/// reaches the hardware before the enable bit.
pub fn apply_program(seq: &ProgramSeq) {
    let mut i: usize = 0;
    while i < SEQ_LEN {
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
        let t = RegionTable {
            base,
            size,
            enabled,
            writable,
        };
        let part: u32 = kani::any();
        kani::assume(part < MAX_PARTITIONS as u32);
        let p = part as usize;
        for r in 0..MAX_REGIONS {
            let i = p * MAX_REGIONS + r;
            if t.enabled[i] {
                kani::assume(validate_region(t.base[i], t.size[i]));
            }
        }
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
                assert!(seq.w[r].rasr & 1 == 0);
                assert!(seq.w[r].rasr == 0);
                assert!(seq.w[r].rbar == 0);
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
                assert!(seq.w[r].rbar == t.base[i]);
                assert!(seq.w[r].rasr & 1 == 1);
                let expect_field = t.size[i].trailing_zeros() - 1;
                assert!((seq.w[r].rasr >> 1) & 0x1F == expect_field);
                let expect_ap = if t.writable[i] { 3u32 } else { 6u32 };
                assert!((seq.w[r].rasr >> 24) & 0x7 == expect_ap);
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
                if r1 != r2 && seq.w[r1].rasr & 1 == 1 && seq.w[r2].rasr & 1 == 1 {
                    let s1 = 1u64 << (((seq.w[r1].rasr >> 1) & 0x1F) + 1);
                    let s2 = 1u64 << (((seq.w[r2].rasr >> 1) & 0x1F) + 1);
                    let b1 = seq.w[r1].rbar as u64;
                    let b2 = seq.w[r2].rbar as u64;
                    assert!(b1 + s1 <= b2 || b2 + s2 <= b1);
                }
            }
        }
    }
    /// k4 — sequence-total + ordered: all 8 region slots emitted exactly
    /// once (slot r at position r, so no slot skipped and no duplicates),
    /// and the single MPU_CTRL enable write is the final element.
    #[kani::proof]
    #[kani::unwind(33)]
    fn iso_sequence_total_and_ordered() {
        let (t, part) = arbitrary_table_and_partition();
        let seq = t.program_partition(part);
        for r in 0..MAX_REGIONS {
            assert!(seq.w[r].rnr == r as u32);
            assert!(seq.w[r].rnr != MPU_CTRL_ID);
        }
        assert!(seq.w[MAX_REGIONS].rnr == MPU_CTRL_ID);
        assert!(seq.w[MAX_REGIONS].rasr == MPU_CTRL_ENABLE);
    }
}
