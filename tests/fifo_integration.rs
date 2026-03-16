//! Integration tests for the FIFO queue.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::error::*;
use gale::fifo::Fifo;

#[test]
fn init_creates_empty_queue() {
    let f = Fifo::init();
    assert_eq!(f.num_items(), 0);
    assert!(f.is_empty());
    assert!(!f.peek_head());
}

#[test]
fn put_increments_count() {
    let mut f = Fifo::init();
    assert_eq!(f.put(), OK);
    assert_eq!(f.num_items(), 1);
    assert!(!f.is_empty());
    assert!(f.peek_head());
}

#[test]
fn get_decrements_count() {
    let mut f = Fifo::init();
    f.put();
    assert_eq!(f.get(), OK);
    assert_eq!(f.num_items(), 0);
    assert!(f.is_empty());
}

#[test]
fn get_empty_returns_eagain() {
    let mut f = Fifo::init();
    assert_eq!(f.get(), EAGAIN);
    assert!(f.is_empty());
}

#[test]
fn put_get_roundtrip() {
    let mut f = Fifo::init();
    assert_eq!(f.put(), OK);
    assert_eq!(f.get(), OK);
    assert!(f.is_empty());
    assert_eq!(f, Fifo::init());
}

#[test]
fn fifo_order_count_tracking() {
    let mut f = Fifo::init();
    // Enqueue 5 items
    for i in 0..5u32 {
        assert_eq!(f.put(), OK);
        assert_eq!(f.num_items(), i + 1);
    }
    // Dequeue 5 items (FIFO order -- we just track count)
    for i in 0..5u32 {
        assert_eq!(f.get(), OK);
        assert_eq!(f.num_items(), 4 - i);
    }
    assert!(f.is_empty());
}

#[test]
fn fill_then_drain() {
    let n = 20u32;
    let mut f = Fifo::init();

    for i in 0..n {
        assert_eq!(f.put(), OK);
        assert_eq!(f.num_items(), i + 1);
    }

    for i in 0..n {
        assert_eq!(f.get(), OK);
        assert_eq!(f.num_items(), n - 1 - i);
    }
    assert!(f.is_empty());
}

#[test]
fn interleaved_put_get() {
    let mut f = Fifo::init();
    // Put 3
    for _ in 0..3 {
        assert_eq!(f.put(), OK);
    }
    assert_eq!(f.num_items(), 3);
    // Get 2
    for _ in 0..2 {
        assert_eq!(f.get(), OK);
    }
    assert_eq!(f.num_items(), 1);
    // Put 4 more
    for _ in 0..4 {
        assert_eq!(f.put(), OK);
    }
    assert_eq!(f.num_items(), 5);
}

#[test]
fn peek_head_reflects_state() {
    let mut f = Fifo::init();
    assert!(!f.peek_head()); // empty

    f.put();
    assert!(f.peek_head()); // has item

    f.get();
    assert!(!f.peek_head()); // empty again
}

#[test]
fn stress_put_get_cycles() {
    let mut f = Fifo::init();
    for _ in 0..100 {
        // Enqueue 8 items
        for _ in 0..8 {
            assert_eq!(f.put(), OK);
        }
        // Drain all
        while f.num_items() > 0 {
            assert_eq!(f.get(), OK);
        }
        assert!(f.is_empty());
    }
}

#[test]
fn single_item_put_get() {
    let mut f = Fifo::init();
    assert_eq!(f.put(), OK);
    assert_eq!(f.num_items(), 1);
    assert_eq!(f.get(), OK);
    assert!(f.is_empty());
}

#[test]
fn double_get_empty_returns_eagain_twice() {
    let mut f = Fifo::init();
    assert_eq!(f.get(), EAGAIN);
    assert_eq!(f.get(), EAGAIN);
    assert!(f.is_empty());
}

#[test]
fn large_count() {
    let mut f = Fifo::init();
    let n = 10_000u32;
    for _ in 0..n {
        assert_eq!(f.put(), OK);
    }
    assert_eq!(f.num_items(), n);
    assert!(!f.is_empty());
    assert!(f.peek_head());
}
