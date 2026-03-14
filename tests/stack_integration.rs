//! Integration tests for the LIFO stack.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::error::*;
use gale::stack::Stack;

#[test]
fn init_valid_capacity() {
    let s = Stack::init(10).unwrap();
    assert_eq!(s.num_used(), 0);
    assert_eq!(s.num_free(), 10);
    assert!(s.is_empty());
    assert!(!s.is_full());
}

#[test]
fn init_rejects_zero() {
    assert_eq!(Stack::init(0), Err(EINVAL));
}

#[test]
fn push_increments_count() {
    let mut s = Stack::init(5).unwrap();
    assert_eq!(s.push(), OK);
    assert_eq!(s.num_used(), 1);
    assert_eq!(s.num_free(), 4);
}

#[test]
fn pop_decrements_count() {
    let mut s = Stack::init(5).unwrap();
    s.push();
    assert_eq!(s.pop(), OK);
    assert_eq!(s.num_used(), 0);
    assert!(s.is_empty());
}

#[test]
fn push_full_returns_enomem() {
    let mut s = Stack::init(2).unwrap();
    s.push();
    s.push();
    assert!(s.is_full());

    assert_eq!(s.push(), ENOMEM);
    assert!(s.is_full());
}

#[test]
fn pop_empty_returns_ebusy() {
    let mut s = Stack::init(3).unwrap();
    assert_eq!(s.pop(), EBUSY);
    assert!(s.is_empty());
}

#[test]
fn push_pop_roundtrip() {
    let mut s = Stack::init(4).unwrap();
    assert_eq!(s.push(), OK);
    assert_eq!(s.pop(), OK);
    assert!(s.is_empty());
    assert_eq!(s, Stack::init(4).unwrap());
}

#[test]
fn fill_then_drain() {
    let cap = 8u32;
    let mut s = Stack::init(cap).unwrap();

    for i in 0..cap {
        assert_eq!(s.push(), OK);
        assert_eq!(s.num_used(), i + 1);
    }
    assert!(s.is_full());

    for i in 0..cap {
        assert_eq!(s.pop(), OK);
        assert_eq!(s.num_used(), cap - 1 - i);
    }
    assert!(s.is_empty());
}

#[test]
fn conservation_invariant() {
    let cap = 10u32;
    let mut s = Stack::init(cap).unwrap();

    for _ in 0..7 {
        s.push();
        assert_eq!(s.num_free() + s.num_used(), cap);
    }
    for _ in 0..4 {
        s.pop();
        assert_eq!(s.num_free() + s.num_used(), cap);
    }
}

#[test]
fn capacity_one() {
    let mut s = Stack::init(1).unwrap();
    assert_eq!(s.push(), OK);
    assert!(s.is_full());
    assert_eq!(s.push(), ENOMEM);
    assert_eq!(s.pop(), OK);
    assert!(s.is_empty());
}

#[test]
fn interleaved_push_pop() {
    let mut s = Stack::init(5).unwrap();
    // Push 3
    for _ in 0..3 {
        assert_eq!(s.push(), OK);
    }
    assert_eq!(s.num_used(), 3);
    // Pop 2
    for _ in 0..2 {
        assert_eq!(s.pop(), OK);
    }
    assert_eq!(s.num_used(), 1);
    // Push 4 (to fill)
    for _ in 0..4 {
        assert_eq!(s.push(), OK);
    }
    assert!(s.is_full());
}

#[test]
fn large_capacity() {
    let s = Stack::init(1_000_000).unwrap();
    assert_eq!(s.capacity(), 1_000_000);
    assert_eq!(s.num_free(), 1_000_000);
}

#[test]
fn stress_push_pop_cycles() {
    let mut s = Stack::init(16).unwrap();
    for _ in 0..100 {
        // Fill half
        for _ in 0..8 {
            assert_eq!(s.push(), OK);
        }
        // Drain all
        while s.num_used() > 0 {
            assert_eq!(s.pop(), OK);
        }
        assert!(s.is_empty());
    }
}
