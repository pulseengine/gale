//! Integration tests for memory domain management.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::error::*;
use gale::mem_domain::{MAX_PARTITIONS, MemDomain, MemPartition};

// ==================================================================
// Init tests
// ==================================================================

#[test]
fn init_creates_empty_domain() {
    let d = MemDomain::init();
    assert_eq!(d.num_partitions_get(), 0);
    for i in 0..MAX_PARTITIONS {
        assert!(d.partition_get(i).is_none());
    }
    assert!(d.has_free_slot());
}

// ==================================================================
// MemPartition validation tests
// ==================================================================

#[test]
fn partition_valid() {
    let p = MemPartition { start: 0x1000, size: 0x2000, attr: 0x7 };
    assert!(p.is_valid_rt());
    assert_eq!(p.end_u64(), 0x3000);
}

#[test]
fn partition_zero_size_invalid() {
    let p = MemPartition { start: 0x1000, size: 0, attr: 0 };
    assert!(!p.is_valid_rt());
}

#[test]
fn partition_overflow_invalid() {
    let p = MemPartition { start: u32::MAX, size: 1, attr: 0 };
    assert!(!p.is_valid_rt());
}

#[test]
fn partition_max_valid() {
    // start=0, size=u32::MAX is valid (end = u32::MAX, no overflow)
    let p = MemPartition { start: 0, size: u32::MAX, attr: 0 };
    assert!(p.is_valid_rt());
    assert_eq!(p.end_u64(), u32::MAX as u64);
}

#[test]
fn partition_overlap_symmetric() {
    let a = MemPartition { start: 0x1000, size: 0x2000, attr: 0 };
    let b = MemPartition { start: 0x2000, size: 0x1000, attr: 0 };
    // a=[0x1000,0x3000), b=[0x2000,0x3000) — overlap
    assert!(a.overlaps(&b));
    assert!(b.overlaps(&a));
}

#[test]
fn partition_no_overlap_adjacent() {
    let a = MemPartition { start: 0x1000, size: 0x1000, attr: 0 };
    let b = MemPartition { start: 0x2000, size: 0x1000, attr: 0 };
    // a=[0x1000,0x2000), b=[0x2000,0x3000) — adjacent, no overlap
    assert!(!a.overlaps(&b));
    assert!(!b.overlaps(&a));
}

#[test]
fn partition_no_overlap_disjoint() {
    let a = MemPartition { start: 0x1000, size: 0x1000, attr: 0 };
    let b = MemPartition { start: 0x5000, size: 0x1000, attr: 0 };
    assert!(!a.overlaps(&b));
    assert!(!b.overlaps(&a));
}

#[test]
fn partition_overlap_contained() {
    let outer = MemPartition { start: 0x1000, size: 0x4000, attr: 0 };
    let inner = MemPartition { start: 0x2000, size: 0x1000, attr: 0 };
    assert!(outer.overlaps(&inner));
    assert!(inner.overlaps(&outer));
}

// ==================================================================
// Add partition tests
// ==================================================================

#[test]
fn add_single_partition() {
    let mut d = MemDomain::init();
    let p = MemPartition { start: 0x2_0000, size: 0x1000, attr: 0x7 };
    let slot = d.add_partition(&p).unwrap();
    assert_eq!(d.num_partitions_get(), 1);
    let got = d.partition_get(slot).unwrap();
    assert_eq!(got.start, 0x2_0000);
    assert_eq!(got.size, 0x1000);
    assert_eq!(got.attr, 0x7);
}

#[test]
fn add_rejects_zero_size() {
    let mut d = MemDomain::init();
    let p = MemPartition { start: 0x1000, size: 0, attr: 0 };
    assert_eq!(d.add_partition(&p), Err(EINVAL));
    assert_eq!(d.num_partitions_get(), 0);
}

#[test]
fn add_rejects_overflow() {
    let mut d = MemDomain::init();
    let p = MemPartition { start: u32::MAX, size: 1, attr: 0 };
    assert_eq!(d.add_partition(&p), Err(EINVAL));
    assert_eq!(d.num_partitions_get(), 0);
}

#[test]
fn add_rejects_overlapping() {
    let mut d = MemDomain::init();
    let p1 = MemPartition { start: 0x1000, size: 0x2000, attr: 0 };
    d.add_partition(&p1).unwrap();

    // Overlapping with p1
    let p2 = MemPartition { start: 0x2000, size: 0x1000, attr: 0 };
    assert_eq!(d.add_partition(&p2), Err(EINVAL));
    assert_eq!(d.num_partitions_get(), 1);
}

#[test]
fn add_accepts_adjacent() {
    let mut d = MemDomain::init();
    let p1 = MemPartition { start: 0x1000, size: 0x1000, attr: 0 };
    let p2 = MemPartition { start: 0x2000, size: 0x1000, attr: 0 };
    d.add_partition(&p1).unwrap();
    d.add_partition(&p2).unwrap();
    assert_eq!(d.num_partitions_get(), 2);
}

#[test]
fn add_fill_all_slots() {
    let mut d = MemDomain::init();
    for i in 0..MAX_PARTITIONS {
        let p = MemPartition {
            start: i * 0x1_0000,
            size: 0x1000,
            attr: 0,
        };
        d.add_partition(&p).unwrap();
    }
    assert_eq!(d.num_partitions_get(), MAX_PARTITIONS);
    assert!(!d.has_free_slot());

    // One more should fail with ENOSPC
    let extra = MemPartition {
        start: MAX_PARTITIONS * 0x1_0000,
        size: 0x1000,
        attr: 0,
    };
    assert_eq!(d.add_partition(&extra), Err(ENOSPC));
}

// ==================================================================
// Remove partition tests
// ==================================================================

#[test]
fn remove_existing_partition() {
    let mut d = MemDomain::init();
    let p = MemPartition { start: 0x4000, size: 0x2000, attr: 0 };
    let slot = d.add_partition(&p).unwrap();
    assert_eq!(d.num_partitions_get(), 1);

    let removed_slot = d.remove_partition(0x4000, 0x2000).unwrap();
    assert_eq!(removed_slot, slot);
    assert_eq!(d.num_partitions_get(), 0);
    assert!(d.partition_get(slot).is_none());
}

#[test]
fn remove_nonexistent_returns_enoent() {
    let mut d = MemDomain::init();
    let p = MemPartition { start: 0x1000, size: 0x1000, attr: 0 };
    d.add_partition(&p).unwrap();

    assert_eq!(d.remove_partition(0x9999, 0x1000), Err(ENOENT));
    assert_eq!(d.num_partitions_get(), 1);
}

#[test]
fn remove_wrong_size_returns_enoent() {
    let mut d = MemDomain::init();
    let p = MemPartition { start: 0x1000, size: 0x1000, attr: 0 };
    d.add_partition(&p).unwrap();

    // Same start, different size
    assert_eq!(d.remove_partition(0x1000, 0x2000), Err(ENOENT));
    assert_eq!(d.num_partitions_get(), 1);
}

// ==================================================================
// Add-remove roundtrip
// ==================================================================

#[test]
fn add_remove_roundtrip() {
    let mut d = MemDomain::init();
    let p = MemPartition { start: 0x8000, size: 0x4000, attr: 0x5 };
    d.add_partition(&p).unwrap();
    assert_eq!(d.num_partitions_get(), 1);

    d.remove_partition(0x8000, 0x4000).unwrap();
    assert_eq!(d.num_partitions_get(), 0);
}

#[test]
fn slot_reuse_after_remove() {
    let mut d = MemDomain::init();
    let p1 = MemPartition { start: 0x1000, size: 0x1000, attr: 0 };
    let p2 = MemPartition { start: 0x3000, size: 0x1000, attr: 0 };
    let slot1 = d.add_partition(&p1).unwrap();
    d.add_partition(&p2).unwrap();

    // Remove p1 — frees slot
    d.remove_partition(0x1000, 0x1000).unwrap();
    assert_eq!(d.num_partitions_get(), 1);

    // Add p3 — should reuse the freed slot
    let p3 = MemPartition { start: 0x5000, size: 0x1000, attr: 0 };
    let slot3 = d.add_partition(&p3).unwrap();
    assert_eq!(slot3, slot1); // reused the freed slot
    assert_eq!(d.num_partitions_get(), 2);
}

// ==================================================================
// Address containment tests
// ==================================================================

#[test]
fn contains_addr_in_partition() {
    let mut d = MemDomain::init();
    let p = MemPartition { start: 0x2000, size: 0x1000, attr: 0 };
    d.add_partition(&p).unwrap();

    assert!(d.contains_addr(0x2000));  // start
    assert!(d.contains_addr(0x2500));  // middle
    assert!(d.contains_addr(0x2FFF));  // last byte
    assert!(!d.contains_addr(0x3000)); // one past end
    assert!(!d.contains_addr(0x1FFF)); // one before start
    assert!(!d.contains_addr(0x0000)); // way before
}

#[test]
fn contains_addr_multiple_partitions() {
    let mut d = MemDomain::init();
    d.add_partition(&MemPartition { start: 0x1000, size: 0x1000, attr: 0 }).unwrap();
    d.add_partition(&MemPartition { start: 0x4000, size: 0x2000, attr: 0 }).unwrap();

    assert!(d.contains_addr(0x1500));
    assert!(d.contains_addr(0x5000));
    assert!(!d.contains_addr(0x3000)); // gap between partitions
}

#[test]
fn contains_addr_empty_domain() {
    let d = MemDomain::init();
    assert!(!d.contains_addr(0x1000));
}

// ==================================================================
// Edge cases
// ==================================================================

#[test]
fn partition_at_zero() {
    let mut d = MemDomain::init();
    let p = MemPartition { start: 0, size: 0x1000, attr: 0 };
    d.add_partition(&p).unwrap();
    assert!(d.contains_addr(0));
    assert!(d.contains_addr(0xFFF));
    assert!(!d.contains_addr(0x1000));
}

#[test]
fn partition_near_max() {
    let mut d = MemDomain::init();
    // Partition ending at u32::MAX: start + size == u32::MAX
    // start = 0xFFFF_F000, size = 0xFFF => end = 0xFFFF_FFFF = u32::MAX
    let start = 0xFFFF_F000u32;
    let size = 0xFFFu32;
    assert_eq!(start as u64 + size as u64, u32::MAX as u64);
    let p = MemPartition { start, size, attr: 0 };
    assert!(p.is_valid_rt());
    d.add_partition(&p).unwrap();
    assert!(d.contains_addr(start));
    assert!(d.contains_addr(u32::MAX - 1));
}

#[test]
fn fill_drain_cycle() {
    let mut d = MemDomain::init();

    // Fill all slots
    for i in 0..MAX_PARTITIONS {
        let p = MemPartition {
            start: i * 0x1_0000,
            size: 0x1000,
            attr: 0,
        };
        d.add_partition(&p).unwrap();
    }
    assert_eq!(d.num_partitions_get(), MAX_PARTITIONS);

    // Drain all
    for i in 0..MAX_PARTITIONS {
        d.remove_partition(i * 0x1_0000, 0x1000).unwrap();
    }
    assert_eq!(d.num_partitions_get(), 0);
}

#[test]
fn interleaved_add_remove() {
    let mut d = MemDomain::init();

    // Add 3 partitions
    d.add_partition(&MemPartition { start: 0x1_0000, size: 0x1000, attr: 0 }).unwrap();
    d.add_partition(&MemPartition { start: 0x2_0000, size: 0x1000, attr: 0 }).unwrap();
    d.add_partition(&MemPartition { start: 0x3_0000, size: 0x1000, attr: 0 }).unwrap();
    assert_eq!(d.num_partitions_get(), 3);

    // Remove middle
    d.remove_partition(0x2_0000, 0x1000).unwrap();
    assert_eq!(d.num_partitions_get(), 2);

    // Add into gap created by removal — different address, now allowed
    d.add_partition(&MemPartition { start: 0x2_0000, size: 0x500, attr: 0 }).unwrap();
    assert_eq!(d.num_partitions_get(), 3);

    // Verify all present
    assert!(d.contains_addr(0x1_0000));
    assert!(d.contains_addr(0x2_0000));
    assert!(d.contains_addr(0x3_0000));
}

#[test]
fn partition_get_out_of_range() {
    let d = MemDomain::init();
    assert!(d.partition_get(MAX_PARTITIONS).is_none());
    assert!(d.partition_get(u32::MAX).is_none());
}

#[test]
fn remove_then_add_overlapping_where_old_was() {
    let mut d = MemDomain::init();
    let p1 = MemPartition { start: 0x1000, size: 0x2000, attr: 0 };
    d.add_partition(&p1).unwrap();

    // Remove p1
    d.remove_partition(0x1000, 0x2000).unwrap();

    // Now add a partition that would have overlapped with p1
    let p2 = MemPartition { start: 0x1500, size: 0x1000, attr: 0 };
    d.add_partition(&p2).unwrap();
    assert_eq!(d.num_partitions_get(), 1);
}
