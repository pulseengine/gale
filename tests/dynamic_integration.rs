//! Integration tests for the dynamic thread pool model.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::dynamic::DynamicPool;
use gale::error::*;

#[test]
fn init_valid() {
    let p = DynamicPool::init(8, 4096).unwrap();
    assert_eq!(p.active_get(), 0);
    assert_eq!(p.available_get(), 8);
    assert_eq!(p.max_threads_get(), 8);
    assert_eq!(p.stack_size_get(), 4096);
    assert!(p.is_empty());
    assert!(!p.is_full());
}

#[test]
fn init_rejects_zero_max() {
    assert_eq!(DynamicPool::init(0, 4096), Err(EINVAL));
}

#[test]
fn init_rejects_zero_stack() {
    assert_eq!(DynamicPool::init(8, 0), Err(EINVAL));
}

#[test]
fn alloc_one() {
    let mut p = DynamicPool::init(4, 2048).unwrap();
    assert_eq!(p.alloc(), OK);
    assert_eq!(p.active_get(), 1);
    assert_eq!(p.available_get(), 3);
}

#[test]
fn alloc_all() {
    let mut p = DynamicPool::init(3, 1024).unwrap();
    for i in 0..3 {
        assert_eq!(p.alloc(), OK);
        assert_eq!(p.active_get(), i + 1);
    }
    assert!(p.is_full());
}

#[test]
fn alloc_when_full_returns_enomem() {
    let mut p = DynamicPool::init(2, 512).unwrap();
    assert_eq!(p.alloc(), OK);
    assert_eq!(p.alloc(), OK);
    assert_eq!(p.alloc(), ENOMEM);
    assert_eq!(p.active_get(), 2);
}

#[test]
fn free_one() {
    let mut p = DynamicPool::init(4, 2048).unwrap();
    p.alloc();
    assert_eq!(p.free(), OK);
    assert_eq!(p.active_get(), 0);
    assert!(p.is_empty());
}

#[test]
fn free_when_empty_returns_einval() {
    let mut p = DynamicPool::init(4, 2048).unwrap();
    assert_eq!(p.free(), EINVAL);
}

#[test]
fn alloc_free_roundtrip() {
    let mut p = DynamicPool::init(4, 2048).unwrap();
    let original = p.clone();
    assert_eq!(p.alloc(), OK);
    assert_eq!(p.free(), OK);
    assert_eq!(p, original);
}

#[test]
fn can_serve_within_limit() {
    let p = DynamicPool::init(4, 4096).unwrap();
    assert!(p.can_serve(1024));
    assert!(p.can_serve(4096));
    assert!(!p.can_serve(4097));
}

#[test]
fn conservation_invariant() {
    let mut p = DynamicPool::init(10, 1024).unwrap();
    for _ in 0..5 {
        p.alloc();
    }
    assert_eq!(p.active_get() + p.available_get(), 10);
    for _ in 0..3 {
        p.free();
    }
    assert_eq!(p.active_get() + p.available_get(), 10);
}

#[test]
fn clone_and_eq() {
    let p1 = DynamicPool::init(8, 4096).unwrap();
    let p2 = p1.clone();
    assert_eq!(p1, p2);

    let mut p3 = p1.clone();
    p3.alloc();
    assert_ne!(p1, p3);
}

#[test]
fn stress_alloc_free_cycles() {
    let mut p = DynamicPool::init(16, 2048).unwrap();
    for _ in 0..100 {
        for _ in 0..16 {
            assert_eq!(p.alloc(), OK);
        }
        assert!(p.is_full());
        for _ in 0..16 {
            assert_eq!(p.free(), OK);
        }
        assert!(p.is_empty());
    }
}

#[test]
fn single_thread_pool() {
    let mut p = DynamicPool::init(1, 8192).unwrap();
    assert_eq!(p.alloc(), OK);
    assert!(p.is_full());
    assert_eq!(p.alloc(), ENOMEM);
    assert_eq!(p.free(), OK);
    assert!(p.is_empty());
}

#[test]
fn can_serve_zero_size() {
    let p = DynamicPool::init(4, 4096).unwrap();
    assert!(p.can_serve(0));
}
