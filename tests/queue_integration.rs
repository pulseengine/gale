//! Integration tests for the dynamic queue.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::error::*;
use gale::queue::Queue;

#[test]
fn init_creates_empty_queue() {
    let q = Queue::init();
    assert_eq!(q.count_get(), 0);
    assert!(q.is_empty());
}

#[test]
fn append_increments_count() {
    let mut q = Queue::init();
    assert_eq!(q.append(), OK);
    assert_eq!(q.count_get(), 1);
    assert!(!q.is_empty());
}

#[test]
fn prepend_increments_count() {
    let mut q = Queue::init();
    assert_eq!(q.prepend(), OK);
    assert_eq!(q.count_get(), 1);
    assert!(!q.is_empty());
}

#[test]
fn get_decrements_count() {
    let mut q = Queue::init();
    q.append();
    assert_eq!(q.get(), OK);
    assert_eq!(q.count_get(), 0);
    assert!(q.is_empty());
}

#[test]
fn get_empty_returns_eagain() {
    let mut q = Queue::init();
    assert_eq!(q.get(), EAGAIN);
    assert!(q.is_empty());
}

#[test]
fn append_get_roundtrip() {
    let mut q = Queue::init();
    assert_eq!(q.append(), OK);
    assert_eq!(q.get(), OK);
    assert!(q.is_empty());
    assert_eq!(q, Queue::init());
}

#[test]
fn prepend_get_roundtrip() {
    let mut q = Queue::init();
    assert_eq!(q.prepend(), OK);
    assert_eq!(q.get(), OK);
    assert!(q.is_empty());
    assert_eq!(q, Queue::init());
}

#[test]
fn multiple_appends_then_gets() {
    let n = 10u32;
    let mut q = Queue::init();

    for i in 0..n {
        assert_eq!(q.append(), OK);
        assert_eq!(q.count_get(), i + 1);
    }

    for i in 0..n {
        assert_eq!(q.get(), OK);
        assert_eq!(q.count_get(), n - 1 - i);
    }
    assert!(q.is_empty());
}

#[test]
fn interleaved_append_prepend_get() {
    let mut q = Queue::init();

    // Append 3
    for _ in 0..3 {
        assert_eq!(q.append(), OK);
    }
    assert_eq!(q.count_get(), 3);

    // Prepend 2
    for _ in 0..2 {
        assert_eq!(q.prepend(), OK);
    }
    assert_eq!(q.count_get(), 5);

    // Get 4
    for _ in 0..4 {
        assert_eq!(q.get(), OK);
    }
    assert_eq!(q.count_get(), 1);

    // Get last
    assert_eq!(q.get(), OK);
    assert!(q.is_empty());

    // Get on empty
    assert_eq!(q.get(), EAGAIN);
}

#[test]
fn overflow_protection() {
    let mut q = Queue { count: u32::MAX };
    assert_eq!(q.append(), EOVERFLOW);
    assert_eq!(q.count_get(), u32::MAX);

    assert_eq!(q.prepend(), EOVERFLOW);
    assert_eq!(q.count_get(), u32::MAX);
}

#[test]
fn stress_append_get_cycles() {
    let mut q = Queue::init();
    for _ in 0..100 {
        // Add 8
        for _ in 0..8 {
            assert_eq!(q.append(), OK);
        }
        // Drain all
        while q.count_get() > 0 {
            assert_eq!(q.get(), OK);
        }
        assert!(q.is_empty());
    }
}

#[test]
fn large_count() {
    let mut q = Queue::init();
    let n = 1000u32;
    for _ in 0..n {
        assert_eq!(q.append(), OK);
    }
    assert_eq!(q.count_get(), n);
    for _ in 0..n {
        assert_eq!(q.get(), OK);
    }
    assert!(q.is_empty());
}

#[test]
fn clone_and_equality() {
    let mut q = Queue::init();
    q.append();
    q.append();
    let q2 = q;
    assert_eq!(q, q2);
    assert_eq!(q.count_get(), q2.count_get());
}
