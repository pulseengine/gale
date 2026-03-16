//! Integration tests for the kernel heap model.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::error::*;
use gale::kheap::KHeap;

#[test]
fn init_valid_capacity() {
    let h = KHeap::init(1024).unwrap();
    assert_eq!(h.allocated_get(), 0);
    assert_eq!(h.free_get(), 1024);
    assert_eq!(h.capacity_get(), 1024);
    assert!(h.is_empty());
    assert!(!h.is_full());
}

#[test]
fn init_rejects_zero_capacity() {
    assert_eq!(KHeap::init(0), Err(EINVAL));
}

#[test]
fn alloc_increments_allocated() {
    let mut h = KHeap::init(1024).unwrap();
    assert_eq!(h.alloc(100), OK);
    assert_eq!(h.allocated_get(), 100);
    assert_eq!(h.free_get(), 924);
}

#[test]
fn alloc_multiple() {
    let mut h = KHeap::init(1024).unwrap();
    assert_eq!(h.alloc(100), OK);
    assert_eq!(h.alloc(200), OK);
    assert_eq!(h.alloc(300), OK);
    assert_eq!(h.allocated_get(), 600);
    assert_eq!(h.free_get(), 424);
}

#[test]
fn alloc_exact_capacity() {
    let mut h = KHeap::init(256).unwrap();
    assert_eq!(h.alloc(256), OK);
    assert!(h.is_full());
    assert_eq!(h.allocated_get(), 256);
    assert_eq!(h.free_get(), 0);
}

#[test]
fn alloc_exceeds_capacity_returns_enomem() {
    let mut h = KHeap::init(100).unwrap();
    assert_eq!(h.alloc(101), ENOMEM);
    assert_eq!(h.allocated_get(), 0);
    assert!(h.is_empty());
}

#[test]
fn alloc_when_full_returns_enomem() {
    let mut h = KHeap::init(64).unwrap();
    assert_eq!(h.alloc(64), OK);
    assert!(h.is_full());

    assert_eq!(h.alloc(1), ENOMEM);
    assert!(h.is_full());
    assert_eq!(h.allocated_get(), 64);
}

#[test]
fn free_decrements_allocated() {
    let mut h = KHeap::init(1024).unwrap();
    h.alloc(300);
    assert_eq!(h.free(100), OK);
    assert_eq!(h.allocated_get(), 200);
    assert_eq!(h.free_get(), 824);
}

#[test]
fn free_all() {
    let mut h = KHeap::init(512).unwrap();
    h.alloc(512);
    assert_eq!(h.free(512), OK);
    assert!(h.is_empty());
    assert_eq!(h.allocated_get(), 0);
}

#[test]
fn free_exceeds_allocated_returns_einval() {
    let mut h = KHeap::init(100).unwrap();
    h.alloc(50);
    assert_eq!(h.free(51), EINVAL);
    assert_eq!(h.allocated_get(), 50);
}

#[test]
fn free_when_empty_returns_einval() {
    let mut h = KHeap::init(100).unwrap();
    assert_eq!(h.free(1), EINVAL);
    assert!(h.is_empty());
}

#[test]
fn alloc_free_roundtrip() {
    let mut h = KHeap::init(256).unwrap();
    assert_eq!(h.alloc(64), OK);
    assert_eq!(h.free(64), OK);
    assert!(h.is_empty());
    assert_eq!(h, KHeap::init(256).unwrap());
}

#[test]
fn conservation_invariant() {
    let capacity = 1000u32;
    let mut h = KHeap::init(capacity).unwrap();

    assert_eq!(h.alloc(200), OK);
    assert_eq!(h.free_get() + h.allocated_get(), capacity);

    assert_eq!(h.alloc(300), OK);
    assert_eq!(h.free_get() + h.allocated_get(), capacity);

    assert_eq!(h.free(100), OK);
    assert_eq!(h.free_get() + h.allocated_get(), capacity);
}

#[test]
fn aligned_alloc_same_as_alloc() {
    let mut h = KHeap::init(256).unwrap();
    assert_eq!(h.aligned_alloc(64, 16), OK);
    assert_eq!(h.allocated_get(), 64);
}

#[test]
fn calloc_basic() {
    let mut h = KHeap::init(1024).unwrap();
    assert_eq!(h.calloc(10, 32), OK);
    assert_eq!(h.allocated_get(), 320);
}

#[test]
fn calloc_zero_num() {
    let mut h = KHeap::init(1024).unwrap();
    assert_eq!(h.calloc(0, 32), ENOMEM);
    assert_eq!(h.allocated_get(), 0);
}

#[test]
fn calloc_zero_size() {
    let mut h = KHeap::init(1024).unwrap();
    assert_eq!(h.calloc(10, 0), ENOMEM);
    assert_eq!(h.allocated_get(), 0);
}

#[test]
fn calloc_overflow() {
    let mut h = KHeap::init(u32::MAX).unwrap();
    // u32::MAX * 2 overflows u32 (but fits u64 check)
    assert_eq!(h.calloc(u32::MAX, 2), ENOMEM);
    assert_eq!(h.allocated_get(), 0);
}

#[test]
fn calloc_exceeds_capacity() {
    let mut h = KHeap::init(100).unwrap();
    assert_eq!(h.calloc(10, 20), ENOMEM); // 200 > 100
    assert_eq!(h.allocated_get(), 0);
}

#[test]
fn interleaved_alloc_free() {
    let mut h = KHeap::init(500).unwrap();

    assert_eq!(h.alloc(100), OK); // 100
    assert_eq!(h.alloc(200), OK); // 300
    assert_eq!(h.free(50), OK); // 250
    assert_eq!(h.alloc(100), OK); // 350
    assert_eq!(h.free(200), OK); // 150
    assert_eq!(h.allocated_get(), 150);
    assert_eq!(h.free_get(), 350);
}

#[test]
fn single_byte_heap() {
    let mut h = KHeap::init(1).unwrap();
    assert_eq!(h.alloc(1), OK);
    assert!(h.is_full());
    assert_eq!(h.alloc(1), ENOMEM);
    assert_eq!(h.free(1), OK);
    assert!(h.is_empty());
}

#[test]
fn large_heap() {
    let h = KHeap::init(u32::MAX).unwrap();
    assert_eq!(h.capacity_get(), u32::MAX);
    assert_eq!(h.free_get(), u32::MAX);
    assert_eq!(h.allocated_get(), 0);
}

#[test]
fn stress_alloc_free_cycles() {
    let mut h = KHeap::init(1000).unwrap();
    for _ in 0..100 {
        // Alloc 10 chunks of 50
        for _ in 0..10 {
            assert_eq!(h.alloc(50), OK);
        }
        assert_eq!(h.allocated_get(), 500);
        // Free all at once
        assert_eq!(h.free(500), OK);
        assert!(h.is_empty());
        assert_eq!(h.free_get() + h.allocated_get(), 1000);
    }
}

#[test]
fn clone_and_eq() {
    let h1 = KHeap::init(512).unwrap();
    let h2 = h1.clone();
    assert_eq!(h1, h2);

    let mut h3 = h1.clone();
    h3.alloc(1);
    assert_ne!(h1, h3);
}
