//! Integration tests for the memory pool model.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::error::*;
use gale::mempool::MemPool;

#[test]
fn init_valid() {
    let p = MemPool::init(10, 64).unwrap();
    assert_eq!(p.allocated_get(), 0);
    assert_eq!(p.free_get(), 10);
    assert_eq!(p.capacity_get(), 10);
    assert_eq!(p.block_size_get(), 64);
    assert!(p.is_empty());
    assert!(!p.is_full());
}

#[test]
fn init_rejects_zero_capacity() {
    assert_eq!(MemPool::init(0, 64), Err(EINVAL));
}

#[test]
fn init_rejects_zero_block_size() {
    assert_eq!(MemPool::init(10, 0), Err(EINVAL));
}

#[test]
fn alloc_one() {
    let mut p = MemPool::init(10, 64).unwrap();
    assert_eq!(p.alloc(), OK);
    assert_eq!(p.allocated_get(), 1);
    assert_eq!(p.free_get(), 9);
}

#[test]
fn alloc_all() {
    let mut p = MemPool::init(5, 32).unwrap();
    for i in 0..5 {
        assert_eq!(p.alloc(), OK);
        assert_eq!(p.allocated_get(), i + 1);
    }
    assert!(p.is_full());
}

#[test]
fn alloc_when_full_returns_enomem() {
    let mut p = MemPool::init(3, 16).unwrap();
    for _ in 0..3 {
        assert_eq!(p.alloc(), OK);
    }
    assert_eq!(p.alloc(), ENOMEM);
    assert_eq!(p.allocated_get(), 3);
}

#[test]
fn free_one() {
    let mut p = MemPool::init(10, 64).unwrap();
    p.alloc();
    assert_eq!(p.free(), OK);
    assert_eq!(p.allocated_get(), 0);
    assert!(p.is_empty());
}

#[test]
fn free_when_empty_returns_einval() {
    let mut p = MemPool::init(10, 64).unwrap();
    assert_eq!(p.free(), EINVAL);
}

#[test]
fn alloc_free_roundtrip() {
    let mut p = MemPool::init(8, 128).unwrap();
    let original = p;
    assert_eq!(p.alloc(), OK);
    assert_eq!(p.free(), OK);
    assert_eq!(p, original);
}

#[test]
fn alloc_many_success() {
    let mut p = MemPool::init(10, 64).unwrap();
    assert_eq!(p.alloc_many(5), OK);
    assert_eq!(p.allocated_get(), 5);
}

#[test]
fn alloc_many_exceeds_returns_enomem() {
    let mut p = MemPool::init(10, 64).unwrap();
    assert_eq!(p.alloc_many(11), ENOMEM);
    assert_eq!(p.allocated_get(), 0);
}

#[test]
fn free_many_success() {
    let mut p = MemPool::init(10, 64).unwrap();
    p.alloc_many(5);
    assert_eq!(p.free_many(3), OK);
    assert_eq!(p.allocated_get(), 2);
}

#[test]
fn free_many_exceeds_returns_einval() {
    let mut p = MemPool::init(10, 64).unwrap();
    p.alloc_many(3);
    assert_eq!(p.free_many(4), EINVAL);
    assert_eq!(p.allocated_get(), 3);
}

#[test]
fn conservation_invariant() {
    let mut p = MemPool::init(20, 32).unwrap();
    for _ in 0..10 {
        p.alloc();
    }
    assert_eq!(p.free_get() + p.allocated_get(), 20);
    for _ in 0..5 {
        p.free();
    }
    assert_eq!(p.free_get() + p.allocated_get(), 20);
}

#[test]
fn total_size_no_overflow() {
    let p = MemPool::init(10, 64).unwrap();
    assert_eq!(p.total_size(), Some(640));
}

#[test]
fn total_size_overflow() {
    let p = MemPool::init(u32::MAX, u32::MAX).unwrap();
    assert_eq!(p.total_size(), None);
}

#[test]
fn single_block_pool() {
    let mut p = MemPool::init(1, 256).unwrap();
    assert_eq!(p.alloc(), OK);
    assert!(p.is_full());
    assert_eq!(p.alloc(), ENOMEM);
    assert_eq!(p.free(), OK);
    assert!(p.is_empty());
}

#[test]
fn clone_and_eq() {
    let p1 = MemPool::init(10, 64).unwrap();
    let p2 = p1;
    assert_eq!(p1, p2);

    let mut p3 = p1;
    p3.alloc();
    assert_ne!(p1, p3);
}

#[test]
fn stress_alloc_free_cycles() {
    let mut p = MemPool::init(100, 32).unwrap();
    for _ in 0..50 {
        for _ in 0..100 {
            assert_eq!(p.alloc(), OK);
        }
        assert!(p.is_full());
        for _ in 0..100 {
            assert_eq!(p.free(), OK);
        }
        assert!(p.is_empty());
        assert_eq!(p.free_get() + p.allocated_get(), 100);
    }
}
