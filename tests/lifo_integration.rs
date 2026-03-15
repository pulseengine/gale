//! Integration tests for the LIFO queue.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::error::*;
use gale::lifo::Lifo;

#[test]
fn init_creates_empty_queue() {
    let q = Lifo::init();
    assert_eq!(q.num_items(), 0);
    assert!(q.is_empty());
}

#[test]
fn put_increments_count() {
    let mut q = Lifo::init();
    assert_eq!(q.put(), OK);
    assert_eq!(q.num_items(), 1);
    assert!(!q.is_empty());
}

#[test]
fn get_decrements_count() {
    let mut q = Lifo::init();
    q.put();
    assert_eq!(q.get(), OK);
    assert_eq!(q.num_items(), 0);
    assert!(q.is_empty());
}

#[test]
fn get_empty_returns_eagain() {
    let mut q = Lifo::init();
    assert_eq!(q.get(), EAGAIN);
    assert!(q.is_empty());
}

#[test]
fn put_get_roundtrip() {
    let mut q = Lifo::init();
    assert_eq!(q.put(), OK);
    assert_eq!(q.get(), OK);
    assert!(q.is_empty());
    assert_eq!(q, Lifo::init());
}

#[test]
fn multiple_puts_then_gets() {
    let n = 10u32;
    let mut q = Lifo::init();

    for i in 0..n {
        assert_eq!(q.put(), OK);
        assert_eq!(q.num_items(), i + 1);
    }

    for i in 0..n {
        assert_eq!(q.get(), OK);
        assert_eq!(q.num_items(), n - 1 - i);
    }
    assert!(q.is_empty());
}

#[test]
fn get_after_drain_returns_eagain() {
    let mut q = Lifo::init();
    for _ in 0..5 {
        q.put();
    }
    for _ in 0..5 {
        assert_eq!(q.get(), OK);
    }
    assert_eq!(q.get(), EAGAIN);
}

#[test]
fn interleaved_put_get() {
    let mut q = Lifo::init();
    // Put 3
    for _ in 0..3 {
        assert_eq!(q.put(), OK);
    }
    assert_eq!(q.num_items(), 3);
    // Get 2
    for _ in 0..2 {
        assert_eq!(q.get(), OK);
    }
    assert_eq!(q.num_items(), 1);
    // Put 4 more
    for _ in 0..4 {
        assert_eq!(q.put(), OK);
    }
    assert_eq!(q.num_items(), 5);
}

#[test]
fn single_item_cycle() {
    let mut q = Lifo::init();
    assert_eq!(q.put(), OK);
    assert!(!q.is_empty());
    assert_eq!(q.get(), OK);
    assert!(q.is_empty());
    assert_eq!(q.get(), EAGAIN);
}

#[test]
fn large_count() {
    let mut q = Lifo::init();
    let n = 10_000u32;
    for _ in 0..n {
        assert_eq!(q.put(), OK);
    }
    assert_eq!(q.num_items(), n);
    assert!(!q.is_empty());
}

#[test]
fn stress_put_get_cycles() {
    let mut q = Lifo::init();
    for _ in 0..100 {
        // Put 8
        for _ in 0..8 {
            assert_eq!(q.put(), OK);
        }
        // Drain all
        while q.num_items() > 0 {
            assert_eq!(q.get(), OK);
        }
        assert!(q.is_empty());
    }
}

#[test]
fn clone_and_eq() {
    let mut q = Lifo::init();
    q.put();
    q.put();
    let q2 = q.clone();
    assert_eq!(q, q2);
    assert_eq!(q.num_items(), q2.num_items());
}

#[test]
fn debug_format() {
    let q = Lifo::init();
    let dbg = format!("{:?}", q);
    assert!(dbg.contains("Lifo"));
    assert!(dbg.contains("count: 0"));
}
