//! The privilege axis (gale#410) through the ordinary test track.
//!
//! Verus proves the region model and Kani cross-checks it, but neither engine
//! runs under `cargo test`, so until now no line of `mpu_switch` was exercised
//! by the test track at all — which is what the coverage gate reported on the
//! PR that added the axis, correctly.
//!
//! These are not a third proof. They are a cheap, fast oracle over the SAME
//! shipped `plain/` code the proofs are about: concrete vectors for every AP
//! encoding, the builder's rejections, and a `program_partition` round-trip
//! whose AP is decoded back OUT of the emitted RASR.

// Same allow-block as tests/mpu_integration.rs: the repo denies these
// crate-wide for SHIPPED code, and a test that cannot index a fixed-size array
// or assert a panic is a test written around the lint rather than the property.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use gale::mpu_switch::{
    RegionTable, MAX_PARTITIONS, MAX_REGIONS, MPU_CTRL_DISABLE, MPU_CTRL_ENABLE, MPU_CTRL_ID,
    UNPRIV_MAX, UNPRIV_NONE, UNPRIV_RO, UNPRIV_SAME,
};

/// AP field (RASR bits 26:24), decoded out of an emitted value.
fn ap_of(rasr: u32) -> u32 {
    (rasr >> 24) & 0x7
}

/// SIZE field (RASR bits 5:1).
fn size_field_of(rasr: u32) -> u32 {
    (rasr >> 1) & 0x1F
}

/// Every encoding the model can emit, as concrete vectors rather than a
/// restatement of the encoder. These five are the whole permission space:
/// ARMv7-M has three more AP values (0 = no access at all, 4 = reserved,
/// 7 = RO both with a different meaning on some revisions) and the model emits
/// none of them.
#[test]
fn every_ap_encoding_is_the_documented_one() {
    let cases: &[(bool, u32, u32)] = &[
        (true, UNPRIV_SAME, 3),
        (false, UNPRIV_SAME, 6),
        (true, UNPRIV_RO, 2),
        (true, UNPRIV_NONE, 1),
        (false, UNPRIV_NONE, 5),
    ];
    for &(writable, unpriv, want_ap) in cases {
        let rasr = gale::mpu_switch::rasr_for(1024, writable, unpriv);
        assert_eq!(
            ap_of(rasr),
            want_ap,
            "writable={writable} unpriv={unpriv} should encode AP {want_ap}"
        );
        // The ENABLE bit is part of every enabled emission, and the SIZE field
        // must not move when only the permission changes.
        assert_eq!(rasr & 1, 1, "ENABLE bit must be set for an enabled region");
        assert_eq!(size_field_of(rasr), 9, "1024 B is SIZE field 9 (log2 - 1)");
    }
}

/// The property the axis exists for, stated over the encoder directly: a
/// supervisor-private marking never produces an encoding that grants
/// unprivileged access, whichever way the privileged half is set.
#[test]
fn unpriv_none_never_grants_unprivileged_access() {
    for &writable in &[true, false] {
        let ap = ap_of(gale::mpu_switch::rasr_for(32, writable, UNPRIV_NONE));
        assert!(ap == 1 || ap == 5, "UNPRIV_NONE emitted AP {ap}");
        // The four encodings that DO grant unprivileged access, named.
        assert!(ap != 3 && ap != 6 && ap != 2 && ap != 0);
    }
}

/// The negative control for the test above, and the reason it is not vacuous:
/// an encoder that returned a deny-everything AP for every region would satisfy
/// it while destroying every tenant's own grant.
#[test]
fn unmarked_regions_stay_unprivileged_accessible() {
    for &(writable, unpriv) in &[(true, UNPRIV_SAME), (false, UNPRIV_SAME), (true, UNPRIV_RO)] {
        let ap = ap_of(gale::mpu_switch::rasr_for(4096, writable, unpriv));
        assert!(ap == 3 || ap == 6 || ap == 2, "unmarked region emitted AP {ap}");
        assert!(ap != 1 && ap != 5);
    }
}

/// `try_add_region` is `try_add_region_perm` with `UNPRIV_SAME` — the
/// compatibility claim the whole design rests on. If this ever diverges, every
/// existing caller silently changed meaning.
#[test]
fn try_add_region_is_unpriv_same() {
    let mut a = RegionTable::new();
    let mut b = RegionTable::new();
    assert!(a.try_add_region(0, 0x2000_0000, 1024, true));
    assert!(b.try_add_region_perm(0, 0x2000_0000, 1024, true, UNPRIV_SAME));
    let sa = a.program_partition(0);
    let sb = b.program_partition(0);
    for i in 0..10 {
        assert_eq!(sa.w[i].rnr, sb.w[i].rnr);
        assert_eq!(sa.w[i].rbar, sb.w[i].rbar);
        assert_eq!(sa.w[i].rasr, sb.w[i].rasr, "emission differs at sequence index {i}");
    }
}

/// `perm_wf`'s exec gate: an unprivileged code outside the defined set, and the
/// one redundant spelling, are both REJECTED rather than normalised — and the
/// table is left untouched (B2).
#[test]
fn ill_formed_permission_pairs_are_rejected_and_change_nothing() {
    let mut t = RegionTable::new();
    // Out of range.
    assert!(!t.try_add_region_perm(0, 0x2000_0000, 1024, true, UNPRIV_MAX));
    assert!(!t.try_add_region_perm(0, 0x2000_0000, 1024, true, 99));
    // Redundant: unprivileged-RO under privileged-RO is AP 6, which
    // UNPRIV_SAME already names. Rejected, not rewritten.
    assert!(!t.try_add_region_perm(0, 0x2000_0000, 1024, false, UNPRIV_RO));
    // Nothing was granted by any of them.
    assert!(!t.covers_addr(0, 0x2000_0000));
    let seq = t.program_partition(0);
    for r in 0..MAX_REGIONS {
        assert_eq!(seq.w[r + 1].rasr, 0, "slot {r} should still be disabled");
    }
    // ...and the valid form of the same request still works, so the rejections
    // above are about the permission and not about the region.
    assert!(t.try_add_region_perm(0, 0x2000_0000, 1024, true, UNPRIV_RO));
    assert!(t.covers_addr(0, 0x2000_0000));
}

/// A supervisor-private grant survives the builder and reaches the emitted
/// sequence with the right encoding — the exec form of B5, decoded out of the
/// RASR rather than read back from the table.
#[test]
fn supervisor_private_grant_reaches_the_emission() {
    let mut t = RegionTable::new();
    // A tenant-reachable region and a supervisor-private one, same partition.
    assert!(t.try_add_region(0, 0x2000_0000, 32 * 1024, true));
    assert!(t.try_add_region_perm(0, 0x2000_8000, 32, true, UNPRIV_NONE));
    let seq = t.program_partition(0);

    // Slot 0 is the tenant's: unprivileged-accessible.
    assert_eq!(seq.w[1].rbar, 0x2000_0000);
    assert_eq!(ap_of(seq.w[1].rasr), 3);
    // Slot 1 is the supervisor's: unprivileged-denied.
    assert_eq!(seq.w[2].rbar, 0x2000_8000);
    assert_eq!(ap_of(seq.w[2].rasr), 1);

    // Both are still granted to the partition — "private" is about the
    // unprivileged half, not about whether the region is mapped.
    assert!(t.covers_addr(0, 0x2000_0000));
    assert!(t.covers_addr(0, 0x2000_8000));
}

/// The permission axis does not disturb the sequence discipline P1–P4: disable
/// first, every hardware slot written exactly once, enable last, and unused
/// slots emitted as RASR 0 rather than left stale.
#[test]
fn sequence_discipline_holds_with_mixed_permissions() {
    let mut t = RegionTable::new();
    assert!(t.try_add_region_perm(0, 0x2000_0000, 1024, true, UNPRIV_SAME));
    assert!(t.try_add_region_perm(0, 0x2000_0400, 1024, true, UNPRIV_RO));
    assert!(t.try_add_region_perm(0, 0x2000_0800, 1024, false, UNPRIV_NONE));
    let seq = t.program_partition(0);

    assert_eq!(seq.w[0].rnr, MPU_CTRL_ID);
    assert_eq!(seq.w[0].rasr, MPU_CTRL_DISABLE);
    for r in 0..MAX_REGIONS {
        assert_eq!(seq.w[r + 1].rnr, r as u32, "slot {r} addressed out of order");
    }
    assert_eq!(seq.w[MAX_REGIONS + 1].rnr, MPU_CTRL_ID);
    assert_eq!(seq.w[MAX_REGIONS + 1].rasr, MPU_CTRL_ENABLE);

    assert_eq!(ap_of(seq.w[1].rasr), 3);
    assert_eq!(ap_of(seq.w[2].rasr), 2);
    assert_eq!(ap_of(seq.w[3].rasr), 5);
    for r in 3..MAX_REGIONS {
        assert_eq!(seq.w[r + 1].rasr, 0, "unused slot {r} must be emitted disabled");
    }
}

/// Permissions are per-slot and per-partition: marking one partition's region
/// supervisor-private says nothing about another partition's.
#[test]
fn permissions_do_not_leak_across_partitions() {
    let mut t = RegionTable::new();
    assert!(t.try_add_region_perm(0, 0x2000_0000, 1024, true, UNPRIV_NONE));
    assert!(t.try_add_region_perm(1, 0x2000_0000, 1024, true, UNPRIV_SAME));
    assert_eq!(ap_of(t.program_partition(0).w[1].rasr), 1);
    assert_eq!(ap_of(t.program_partition(1).w[1].rasr), 3);
    assert!(MAX_PARTITIONS >= 2);
}
