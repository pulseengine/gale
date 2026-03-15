//! Integration tests for the memory slab allocator.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::error::*;
use gale::mem_slab::MemSlab;

#[test]
fn init_valid_params() {
    let s = MemSlab::init(64, 10).unwrap();
    assert_eq!(s.num_used_get(), 0);
    assert_eq!(s.num_free_get(), 10);
    assert_eq!(s.num_blocks_get(), 10);
    assert_eq!(s.block_size_get(), 64);
    assert!(s.is_empty());
    assert!(!s.is_full());
}

#[test]
fn init_rejects_zero_block_size() {
    assert_eq!(MemSlab::init(0, 10), Err(EINVAL));
}

#[test]
fn init_rejects_zero_num_blocks() {
    assert_eq!(MemSlab::init(64, 0), Err(EINVAL));
}

#[test]
fn init_rejects_both_zero() {
    assert_eq!(MemSlab::init(0, 0), Err(EINVAL));
}

#[test]
fn alloc_increments_num_used() {
    let mut s = MemSlab::init(32, 5).unwrap();
    assert_eq!(s.alloc(), OK);
    assert_eq!(s.num_used_get(), 1);
    assert_eq!(s.num_free_get(), 4);
}

#[test]
fn alloc_when_full_returns_enomem() {
    let mut s = MemSlab::init(16, 2).unwrap();
    s.alloc();
    s.alloc();
    assert!(s.is_full());

    assert_eq!(s.alloc(), ENOMEM);
    assert!(s.is_full());
    assert_eq!(s.num_used_get(), 2);
}

#[test]
fn free_decrements_num_used() {
    let mut s = MemSlab::init(32, 5).unwrap();
    s.alloc();
    assert_eq!(s.free(), OK);
    assert_eq!(s.num_used_get(), 0);
    assert!(s.is_empty());
}

#[test]
fn free_when_all_free_returns_einval() {
    let mut s = MemSlab::init(32, 3).unwrap();
    assert_eq!(s.free(), EINVAL);
    assert!(s.is_empty());
    assert_eq!(s.num_used_get(), 0);
}

#[test]
fn alloc_free_roundtrip() {
    let mut s = MemSlab::init(64, 4).unwrap();
    assert_eq!(s.alloc(), OK);
    assert_eq!(s.free(), OK);
    assert!(s.is_empty());
    assert_eq!(s, MemSlab::init(64, 4).unwrap());
}

#[test]
fn fill_then_drain() {
    let num_blocks = 8u32;
    let mut s = MemSlab::init(128, num_blocks).unwrap();

    for i in 0..num_blocks {
        assert_eq!(s.alloc(), OK);
        assert_eq!(s.num_used_get(), i + 1);
    }
    assert!(s.is_full());

    for i in 0..num_blocks {
        assert_eq!(s.free(), OK);
        assert_eq!(s.num_used_get(), num_blocks - 1 - i);
    }
    assert!(s.is_empty());
}

#[test]
fn conservation_invariant() {
    let num_blocks = 10u32;
    let mut s = MemSlab::init(64, num_blocks).unwrap();

    for _ in 0..7 {
        s.alloc();
        assert_eq!(s.num_free_get() + s.num_used_get(), num_blocks);
    }
    for _ in 0..4 {
        s.free();
        assert_eq!(s.num_free_get() + s.num_used_get(), num_blocks);
    }
}

#[test]
fn single_block_slab() {
    let mut s = MemSlab::init(256, 1).unwrap();
    assert_eq!(s.alloc(), OK);
    assert!(s.is_full());
    assert_eq!(s.alloc(), ENOMEM);
    assert_eq!(s.free(), OK);
    assert!(s.is_empty());
}

#[test]
fn interleaved_alloc_free() {
    let mut s = MemSlab::init(32, 5).unwrap();
    // Alloc 3
    for _ in 0..3 {
        assert_eq!(s.alloc(), OK);
    }
    assert_eq!(s.num_used_get(), 3);
    // Free 2
    for _ in 0..2 {
        assert_eq!(s.free(), OK);
    }
    assert_eq!(s.num_used_get(), 1);
    // Alloc 4 (to fill)
    for _ in 0..4 {
        assert_eq!(s.alloc(), OK);
    }
    assert!(s.is_full());
}

#[test]
fn large_slab() {
    let s = MemSlab::init(4096, 1_000_000).unwrap();
    assert_eq!(s.num_blocks_get(), 1_000_000);
    assert_eq!(s.block_size_get(), 4096);
    assert_eq!(s.num_free_get(), 1_000_000);
}

#[test]
fn stress_alloc_free_cycles() {
    let mut s = MemSlab::init(64, 16).unwrap();
    for _ in 0..100 {
        // Fill half
        for _ in 0..8 {
            assert_eq!(s.alloc(), OK);
        }
        // Drain all
        while s.num_used_get() > 0 {
            assert_eq!(s.free(), OK);
        }
        assert!(s.is_empty());
        assert_eq!(s.num_free_get() + s.num_used_get(), 16);
    }
}
