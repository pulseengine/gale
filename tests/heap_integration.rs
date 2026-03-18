//! Integration tests for the sys_heap chunk allocator model.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::error::*;
use gale::heap::{Heap, MAX_CHUNKS};

// =====================================================================
// Init tests
// =====================================================================

#[test]
fn init_valid() {
    let h = Heap::init(1024, 64).unwrap();
    assert_eq!(h.capacity_get(), 1024);
    assert_eq!(h.allocated_get(), 64); // overhead
    assert_eq!(h.free_bytes_get(), 960);
    assert_eq!(h.total_chunks_get(), 2);
    assert_eq!(h.free_chunks_get(), 1);
    assert_eq!(h.used_chunks_get(), 1);
    assert!(!h.is_full());
    assert!(!h.is_empty());
}

#[test]
fn init_rejects_zero_capacity() {
    assert_eq!(Heap::init(0, 32), Err(EINVAL));
}

#[test]
fn init_rejects_zero_overhead() {
    assert_eq!(Heap::init(1024, 0), Err(EINVAL));
}

#[test]
fn init_rejects_overhead_equals_capacity() {
    assert_eq!(Heap::init(64, 64), Err(EINVAL));
}

#[test]
fn init_rejects_overhead_exceeds_capacity() {
    assert_eq!(Heap::init(32, 64), Err(EINVAL));
}

// =====================================================================
// Alloc tests — HP1, HP3, HP5, HP7
// =====================================================================

#[test]
fn alloc_basic() {
    let mut h = Heap::init(1024, 64).unwrap();
    let slot = h.alloc(100).unwrap();
    assert_eq!(slot, 1); // first slot after chunk0
    assert_eq!(h.allocated_get(), 164);
    assert_eq!(h.free_bytes_get(), 860);
    assert_eq!(h.free_chunks_get(), 0);
}

#[test]
fn alloc_exact_remaining() {
    let mut h = Heap::init(256, 56).unwrap();
    let slot = h.alloc(200).unwrap();
    assert_eq!(slot, 1);
    assert!(h.is_full());
    assert_eq!(h.allocated_get(), 256);
    assert_eq!(h.free_bytes_get(), 0);
}

#[test]
fn alloc_exceeds_capacity_returns_enomem() {
    let mut h = Heap::init(100, 20).unwrap();
    assert!(h.alloc(81).is_err());
    assert_eq!(h.allocated_get(), 20); // unchanged
    assert_eq!(h.free_chunks_get(), 1);
}

#[test]
fn alloc_no_free_chunks_returns_enomem() {
    let mut h = Heap::init(1024, 64).unwrap();
    // Use the one free chunk
    h.alloc(100).unwrap();
    assert_eq!(h.free_chunks_get(), 0);
    // Now alloc fails even though bytes might fit
    assert!(h.alloc(1).is_err());
}

#[test]
fn alloc_slot_ids_monotonic() {
    let mut h = Heap::init(1024, 64).unwrap();
    // Need free chunks for each alloc — add splits
    let s1 = h.alloc(50).unwrap();
    h.split(50, 100); // create new free chunk
    let s2 = h.alloc(50).unwrap();
    assert_eq!(s1, 1);
    assert_eq!(s2, 2);
    assert!(s2 > s1);
}

// =====================================================================
// Free tests — HP4, HP5
// =====================================================================

#[test]
fn free_basic() {
    let mut h = Heap::init(1024, 64).unwrap();
    h.alloc(100).unwrap();
    assert_eq!(h.free(100), OK);
    assert_eq!(h.allocated_get(), 64);
    assert_eq!(h.free_chunks_get(), 1);
}

#[test]
fn free_exceeds_allocated_returns_einval() {
    let mut h = Heap::init(1024, 64).unwrap();
    h.alloc(100).unwrap();
    assert_eq!(h.free(200), EINVAL);
    assert_eq!(h.allocated_get(), 164);
}

#[test]
fn double_free_rejected() {
    // HP5: when all chunks are already free, further free is rejected.
    // After init: total_chunks=2, free_chunks=1 (chunk0 always used).
    // After alloc(100): free_chunks=0.
    // After free(100): free_chunks=1 (back to init state).
    // After free(64): free_chunks=2 == total_chunks => next free is double-free.
    let mut h = Heap::init(1024, 64).unwrap();
    h.alloc(100).unwrap();
    assert_eq!(h.free(100), OK);
    // free_chunks=1, total_chunks=2, allocated=64. Free the overhead too:
    assert_eq!(h.free(64), OK);
    // Now free_chunks=2 == total_chunks=2 => double-free detected
    assert_eq!(h.free(1), EINVAL);
}

#[test]
fn free_when_empty_returns_einval() {
    let mut h = Heap::init(1024, 64).unwrap();
    // Only chunk0 is used, free_chunks == total_chunks - 1
    // But if we try to free more than allocated
    assert_eq!(h.free(65), EINVAL);
}

// =====================================================================
// Alloc-free roundtrip — HP3 + HP4
// =====================================================================

#[test]
fn alloc_free_roundtrip() {
    let mut h = Heap::init(512, 32).unwrap();
    let original = h;
    let slot = h.alloc(100).unwrap();
    assert_eq!(slot, 1);
    assert_eq!(h.free(100), OK);
    assert_eq!(h.allocated_get(), original.allocated_get());
    assert_eq!(h.free_chunks_get(), original.free_chunks_get());
}

#[test]
fn conservation_through_ops() {
    let capacity = 1000u32;
    let overhead = 80u32;
    let mut h = Heap::init(capacity, overhead).unwrap();

    h.alloc(200).unwrap();
    assert_eq!(h.free_bytes_get() + h.allocated_get(), capacity);

    h.free(100);
    assert_eq!(h.free_bytes_get() + h.allocated_get(), capacity);

    assert_eq!(
        h.free_chunks_get() + h.used_chunks_get(),
        h.total_chunks_get()
    );
}

// =====================================================================
// Split tests — HP8
// =====================================================================

#[test]
fn split_increases_chunks() {
    let mut h = Heap::init(1024, 64).unwrap();
    assert_eq!(h.total_chunks_get(), 2);
    assert_eq!(h.split(32, 64), OK);
    assert_eq!(h.total_chunks_get(), 3);
    assert_eq!(h.free_chunks_get(), 2);
    // Allocated bytes unchanged
    assert_eq!(h.allocated_get(), 64);
}

#[test]
fn split_at_max_chunks_rejected() {
    let mut h = Heap::init(1024, 64).unwrap();
    h.total_chunks = MAX_CHUNKS;
    h.free_chunks = MAX_CHUNKS - 1;
    assert_eq!(h.split(32, 64), EINVAL);
}

// =====================================================================
// Merge tests — HP8
// =====================================================================

#[test]
fn merge_decreases_chunks() {
    let mut h = Heap::init(1024, 64).unwrap();
    // Start with 2 chunks (1 free). Split to get 3 (2 free).
    h.split(32, 64);
    assert_eq!(h.total_chunks_get(), 3);
    assert_eq!(h.free_chunks_get(), 2);

    assert_eq!(h.merge(), OK);
    assert_eq!(h.total_chunks_get(), 2);
    assert_eq!(h.free_chunks_get(), 1);
}

#[test]
fn split_merge_roundtrip() {
    let mut h = Heap::init(1024, 64).unwrap();
    let original = h;
    h.split(32, 64);
    h.merge();
    assert_eq!(h.total_chunks_get(), original.total_chunks_get());
    assert_eq!(h.free_chunks_get(), original.free_chunks_get());
    assert_eq!(h.allocated_get(), original.allocated_get());
}

// =====================================================================
// Coalesce free tests — HP8
// =====================================================================

#[test]
fn coalesce_no_merge() {
    let mut h = Heap::init(1024, 64).unwrap();
    h.alloc(100).unwrap();
    // Now: 2 chunks, 0 free, 164 allocated
    // Need a free chunk neighbor scenario — add a split first
    h.split(50, 100);
    // Now: 3 chunks, 1 free, 164 allocated
    // Free 100 bytes without merging neighbors
    assert_eq!(h.coalesce_free(100, false, false), OK);
    assert_eq!(h.allocated_get(), 64);
    assert_eq!(h.free_chunks_get(), 2);
}

#[test]
fn coalesce_merge_right() {
    let mut h = Heap::init(1024, 64).unwrap();
    h.alloc(100).unwrap();
    h.split(50, 100);
    // 3 chunks, 1 free, 164 allocated
    assert_eq!(h.coalesce_free(100, false, true), OK);
    assert_eq!(h.allocated_get(), 64);
    // freed+1 then merged-1 = net +0 free, but total-1
    assert_eq!(h.free_chunks_get(), 1);
    assert_eq!(h.total_chunks_get(), 2);
}

#[test]
fn coalesce_merge_both() {
    let mut h = Heap::init(1024, 64).unwrap();
    h.alloc(100).unwrap();
    h.split(50, 100);
    h.split(25, 50);
    // 4 chunks, 2 free, 164 allocated
    assert_eq!(h.coalesce_free(100, true, true), OK);
    assert_eq!(h.allocated_get(), 64);
}

// =====================================================================
// Aligned alloc tests — HP6
// =====================================================================

#[test]
fn aligned_alloc_small_align_same_as_regular() {
    let mut h1 = Heap::init(1024, 64).unwrap();
    let mut h2 = Heap::init(1024, 64).unwrap();

    let s1 = h1.alloc(100).unwrap();
    let s2 = h2.aligned_alloc(100, 0).unwrap();
    assert_eq!(s1, s2);
    assert_eq!(h1.allocated_get(), h2.allocated_get());
}

#[test]
fn aligned_alloc_with_padding() {
    let mut h = Heap::init(1024, 64).unwrap();
    // align=64, CHUNK_UNIT=8, padding = 64-8 = 56
    // padded = 100 + 56 = 156
    let slot = h.aligned_alloc(100, 64).unwrap();
    assert_eq!(slot, 1);
    assert_eq!(h.allocated_get(), 64 + 156); // overhead + padded
}

#[test]
fn aligned_alloc_overflow_rejected() {
    let mut h = Heap::init(u32::MAX, 64).unwrap();
    // bytes = u32::MAX - 100, align = u32::MAX (power of 2 not required
    // for overflow test; just needs to cause u64 overflow check)
    // Actually need align that causes padded > u32::MAX
    // align = 2^31 = 2147483648, padding = 2147483648 - 8 = 2147483640
    // bytes = u32::MAX - 100 = 4294967195
    // padded = 4294967195 + 2147483640 > u32::MAX
    assert!(h.aligned_alloc(u32::MAX - 100, 2_147_483_648).is_err());
}

// =====================================================================
// Realloc tests
// =====================================================================

#[test]
fn realloc_shrink() {
    let mut h = Heap::init(1024, 64).unwrap();
    h.alloc(200).unwrap();
    assert_eq!(h.allocated_get(), 264);

    let r = h.realloc(200, 100).unwrap();
    assert_eq!(r, 0);
    assert_eq!(h.allocated_get(), 164);
}

#[test]
fn realloc_same_size() {
    let mut h = Heap::init(1024, 64).unwrap();
    h.alloc(200).unwrap();
    let before = h.allocated_get();

    let r = h.realloc(200, 200).unwrap();
    assert_eq!(r, 0);
    assert_eq!(h.allocated_get(), before);
}

#[test]
fn realloc_grow_with_space() {
    let mut h = Heap::init(1024, 64).unwrap();
    h.alloc(200).unwrap();
    assert_eq!(h.allocated_get(), 264);

    let r = h.realloc(200, 400).unwrap();
    assert_eq!(r, 0);
    assert_eq!(h.allocated_get(), 464);
}

#[test]
fn realloc_grow_no_space() {
    let mut h = Heap::init(300, 64).unwrap();
    h.alloc(200).unwrap();
    assert_eq!(h.allocated_get(), 264);

    assert!(h.realloc(200, 300).is_err());
    assert_eq!(h.allocated_get(), 264); // unchanged
}

// =====================================================================
// bytes_to_chunks — HP7
// =====================================================================

#[test]
fn bytes_to_chunks_basic() {
    assert_eq!(Heap::bytes_to_chunks(0), 0);
    assert_eq!(Heap::bytes_to_chunks(1), 1);
    assert_eq!(Heap::bytes_to_chunks(7), 1);
    assert_eq!(Heap::bytes_to_chunks(8), 1);
    assert_eq!(Heap::bytes_to_chunks(9), 2);
    assert_eq!(Heap::bytes_to_chunks(16), 2);
    assert_eq!(Heap::bytes_to_chunks(17), 3);
}

#[test]
fn bytes_to_chunks_large() {
    // u32::MAX = 4294967295
    // (4294967295 + 7) / 8 = 4294967302 / 8 = 536870912 (truncated)
    let r = Heap::bytes_to_chunks(u32::MAX);
    assert!(r > 0);
    // Verify no panic/overflow
    assert_eq!(r, 536_870_912);
}

// =====================================================================
// Chunk conservation — HP2
// =====================================================================

#[test]
fn chunk_conservation_through_lifecycle() {
    let mut h = Heap::init(1024, 64).unwrap();

    // Initial: used + free == total
    assert_eq!(
        h.used_chunks_get() + h.free_chunks_get(),
        h.total_chunks_get()
    );

    // After alloc
    h.alloc(100).unwrap();
    assert_eq!(
        h.used_chunks_get() + h.free_chunks_get(),
        h.total_chunks_get()
    );

    // After split
    h.split(50, 100);
    assert_eq!(
        h.used_chunks_get() + h.free_chunks_get(),
        h.total_chunks_get()
    );

    // After free
    h.free(100);
    assert_eq!(
        h.used_chunks_get() + h.free_chunks_get(),
        h.total_chunks_get()
    );

    // After merge
    if h.free_chunks_get() > 1 && h.total_chunks_get() > 1 {
        h.merge();
        assert_eq!(
            h.used_chunks_get() + h.free_chunks_get(),
            h.total_chunks_get()
        );
    }
}

// =====================================================================
// Stress tests
// =====================================================================

#[test]
fn stress_alloc_split_free_merge() {
    let mut h = Heap::init(10000, 100).unwrap();

    for i in 0u32..50 {
        // Alloc
        if h.free_chunks_get() > 0 && (i * 10 + 10) <= h.free_bytes_get() {
            let _ = h.alloc(i * 10 + 10);
        }
        // Split to create more free chunks
        if h.total_chunks_get() < MAX_CHUNKS {
            h.split(10, 20);
        }

        // Conservation always holds
        assert_eq!(
            h.used_chunks_get() + h.free_chunks_get(),
            h.total_chunks_get()
        );
        assert_eq!(h.free_bytes_get() + h.allocated_get(), h.capacity_get());
    }
}

#[test]
fn clone_and_eq() {
    let h1 = Heap::init(512, 32).unwrap();
    let h2 = h1;
    assert_eq!(h1, h2);

    let mut h3 = h1;
    h3.alloc(1).unwrap();
    assert_ne!(h1, h3);
}

// =====================================================================
// Edge cases
// =====================================================================

#[test]
fn minimum_heap() {
    // Smallest possible: capacity=2, overhead=1
    let h = Heap::init(2, 1).unwrap();
    assert_eq!(h.capacity_get(), 2);
    assert_eq!(h.allocated_get(), 1);
    assert_eq!(h.free_bytes_get(), 1);
}

#[test]
fn alloc_then_realloc_shrink_to_zero() {
    let mut h = Heap::init(1024, 64).unwrap();
    h.alloc(200).unwrap();
    // Realloc shrink: 200 -> 1 (minimum non-zero)
    let r = h.realloc(200, 1).unwrap();
    assert_eq!(r, 0);
    assert_eq!(h.allocated_get(), 64 + 1);
}
