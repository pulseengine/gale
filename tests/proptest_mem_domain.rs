//! Property-based tests for memory domain management.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_possible_truncation,
    clippy::checked_conversions,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::error::*;
use gale::mem_domain::{MAX_PARTITIONS, MemDomain, MemPartition};
use proptest::prelude::*;

/// Generate a valid (non-overlapping, non-zero-size, no-overflow) partition
/// in a given address range.
fn valid_partition_strategy() -> impl Strategy<Value = MemPartition> {
    // Use 24-bit addresses to leave room for sizes without overflow
    (0u32..0x00FF_0000, 1u32..0x1_0000, any::<u32>()).prop_map(|(start, size, attr)| {
        MemPartition { start, size, attr }
    })
}

/// Generate a list of non-overlapping partitions.
fn non_overlapping_partitions(max_count: usize) -> impl Strategy<Value = Vec<MemPartition>> {
    // Generate partitions at well-separated addresses to guarantee no overlap
    proptest::collection::vec(0u32..256, 1..=max_count).prop_map(|offsets| {
        let mut parts = Vec::new();
        for (i, _) in offsets.iter().enumerate() {
            if i >= MAX_PARTITIONS as usize {
                break;
            }
            // Each partition at i * 0x1_0000 with size 0x1000 — guaranteed no overlap
            let start = (i as u32) * 0x1_0000;
            let p = MemPartition { start, size: 0x1000, attr: 0 };
            parts.push(p);
        }
        parts
    })
}

proptest! {
    /// MD1: no two active partitions overlap after arbitrary add operations.
    #[test]
    fn non_overlap_maintained(
        parts in non_overlapping_partitions(16)
    ) {
        let mut d = MemDomain::init();
        for p in &parts {
            d.add_partition(p).unwrap();
        }
        // Verify no two active partitions overlap
        for i in 0..MAX_PARTITIONS {
            let pi = d.partition_get(i);
            if pi.is_none() { continue; }
            let pi = pi.unwrap();
            for j in (i + 1)..MAX_PARTITIONS {
                let pj = d.partition_get(j);
                if pj.is_none() { continue; }
                let pj = pj.unwrap();
                prop_assert!(
                    !pi.overlaps(&pj),
                    "partitions at slots {} and {} overlap: {:?} vs {:?}",
                    i, j, pi, pj
                );
            }
        }
    }

    /// MD3: all active partitions have size > 0.
    #[test]
    fn active_partitions_nonzero_size(
        parts in non_overlapping_partitions(16)
    ) {
        let mut d = MemDomain::init();
        for p in &parts {
            d.add_partition(p).unwrap();
        }
        for i in 0..MAX_PARTITIONS {
            if let Some(p) = d.partition_get(i) {
                prop_assert!(p.size > 0);
            }
        }
    }

    /// MD4: num_partitions <= MAX_PARTITIONS always holds.
    #[test]
    fn num_partitions_bounded(
        parts in non_overlapping_partitions(16)
    ) {
        let mut d = MemDomain::init();
        for p in &parts {
            let _ = d.add_partition(p);
            prop_assert!(d.num_partitions_get() <= MAX_PARTITIONS);
        }
    }

    /// MD5: add-remove roundtrip preserves num_partitions.
    #[test]
    fn add_remove_roundtrip(
        parts in non_overlapping_partitions(8)
    ) {
        let mut d = MemDomain::init();
        // Add all
        for p in &parts {
            d.add_partition(p).unwrap();
        }
        let count_after_add = d.num_partitions_get();
        prop_assert_eq!(count_after_add, parts.len() as u32);

        // Remove all
        for p in &parts {
            d.remove_partition(p.start, p.size).unwrap();
        }
        prop_assert_eq!(d.num_partitions_get(), 0);
    }

    /// MD6: valid partitions have no address overflow.
    #[test]
    fn valid_partition_no_overflow(p in valid_partition_strategy()) {
        if p.is_valid_rt() {
            let end = p.start as u64 + p.size as u64;
            prop_assert!(end <= u32::MAX as u64);
        }
    }

    /// Overlap detection is symmetric.
    #[test]
    fn overlap_symmetric(
        a in valid_partition_strategy(),
        b in valid_partition_strategy()
    ) {
        prop_assert_eq!(a.overlaps(&b), b.overlaps(&a));
    }

    /// Rejected adds don't modify state.
    #[test]
    fn rejected_add_preserves_state(
        valid_parts in non_overlapping_partitions(4)
    ) {
        let mut d = MemDomain::init();
        for p in &valid_parts {
            d.add_partition(p).unwrap();
        }
        let count_before = d.num_partitions_get();

        // Try adding invalid partitions — should fail without changing state
        let invalid_cases = [
            MemPartition { start: 0, size: 0, attr: 0 },        // zero size
            MemPartition { start: u32::MAX, size: 1, attr: 0 }, // overflow
        ];
        for bad in &invalid_cases {
            let result = d.add_partition(bad);
            prop_assert!(result.is_err());
            prop_assert_eq!(d.num_partitions_get(), count_before);
        }
    }

    /// Rejected removes don't modify state.
    #[test]
    fn rejected_remove_preserves_state(
        parts in non_overlapping_partitions(4)
    ) {
        let mut d = MemDomain::init();
        for p in &parts {
            d.add_partition(p).unwrap();
        }
        let count_before = d.num_partitions_get();

        // Remove nonexistent partition
        let result = d.remove_partition(0xDEAD_0000, 0x1000);
        prop_assert_eq!(result, Err(ENOENT));
        prop_assert_eq!(d.num_partitions_get(), count_before);
    }

    /// contains_addr correctly identifies addresses in partitions.
    #[test]
    fn contains_addr_correct(
        parts in non_overlapping_partitions(8),
        addr_offset in 0u32..0x1000
    ) {
        let mut d = MemDomain::init();
        for p in &parts {
            d.add_partition(p).unwrap();
        }
        // Every address within a partition should be contained
        for p in &parts {
            let test_addr = p.start + (addr_offset % p.size);
            prop_assert!(
                d.contains_addr(test_addr),
                "addr 0x{:x} should be in partition [{:x}, {:x})",
                test_addr, p.start, p.start + p.size
            );
        }
    }

    /// Addresses outside all partitions are not contained.
    #[test]
    fn does_not_contain_gap_addresses(
        parts in non_overlapping_partitions(4)
    ) {
        let mut d = MemDomain::init();
        for p in &parts {
            d.add_partition(p).unwrap();
        }
        // Addresses in gaps between partitions should not be contained
        // Gap addresses: i * 0x1_0000 + 0x1000 .. (i+1) * 0x1_0000
        for i in 0..parts.len() {
            let gap_addr = (i as u32) * 0x1_0000 + 0x1000; // just past partition end
            prop_assert!(
                !d.contains_addr(gap_addr),
                "gap addr 0x{:x} should NOT be in any partition",
                gap_addr
            );
        }
    }

    /// Overlapping add is rejected even after removes.
    #[test]
    fn overlap_rejected_after_partial_remove(
        count in 2usize..=8
    ) {
        let mut d = MemDomain::init();
        // Add `count` non-overlapping partitions
        for i in 0..count {
            let p = MemPartition {
                start: (i as u32) * 0x1_0000,
                size: 0x1000,
                attr: 0,
            };
            d.add_partition(&p).unwrap();
        }

        // Remove the first one
        d.remove_partition(0, 0x1000).unwrap();

        // Try adding one that overlaps with partition at index 1
        let overlap = MemPartition {
            start: 0x1_0000 - 0x500,  // overlaps [0x1_0000, 0x1_1000)
            size: 0x1000,
            attr: 0,
        };
        prop_assert_eq!(d.add_partition(&overlap), Err(EINVAL));
    }
}
