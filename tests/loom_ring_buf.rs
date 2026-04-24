//! Loom permutation tests for a single-producer/single-consumer (SPSC)
//! lock-free ring buffer modelled on `gale::ring_buf::RingBuf`.
//!
//! Property targeted:
//!   * Publication safety — the consumer never reads a slot before the
//!     producer has fully written it (the release/acquire pairing on `tail`
//!     must synchronise-with the data store).
//!   * Round-trip correctness — values pushed and popped across threads
//!     must read back unchanged, with the ring returning to empty.
//!   * FIFO order is preserved on every legal interleaving.
//!
//! Why loom catches what Verus cannot:
//!   `plain/src/ring_buf.rs` is a sequential index arithmetic model —
//!   every method takes `&mut self`, so Verus never reasons about the
//!   producer / consumer running concurrently.  A real port that uses
//!   `AtomicU32` for `head`/`tail` can satisfy Verus on the arithmetic yet
//!   still expose a partial write: the consumer reads the new `tail`
//!   before the data store has been flushed, or the producer observes a
//!   stale `head` (torn publish).  Loom explores every allowed reordering
//!   under the C11 memory model (closely approximating the ARMv8
//!   weak-memory machine) and would flag, e.g., a release/acquire pair
//!   accidentally downgraded to relaxed.
//!
//! How to run:
//! ```text
//! cargo test --test loom_ring_buf \
//!   --config 'target."cfg(all())".rustflags = ["--cfg", "loom"]' \
//!   --config 'env.LOOM_MAX_PREEMPTIONS = "3"' -- --test-threads=1
//! ```
//!
//! Budget: capacity = 2, <= CAP ops per thread, LOOM_MAX_PREEMPTIONS=3.
//! The classic "fill and drain with retries" permutation explodes the
//! search because loom does not tolerate spin loops (see report). These
//! tests avoid that by restricting each thread to at most `CAP`
//! non-retrying operations; anything that thread misses is drained by the
//! main thread after join.
//!
//! Traceability: operationalises the invariants proven in
//! `proofs/lean/RingBuf.lean` (9 index invariants) on an actual atomic
//! implementation.

#![cfg(loom)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    unsafe_code
)]

extern crate alloc;

use loom::cell::UnsafeCell;
use loom::sync::Arc;
use loom::sync::atomic::{AtomicUsize, Ordering};
use loom::thread;

/// Fixed-capacity SPSC ring modelled on Zephyr's `struct ring_buf` index
/// state machine. Uses a power-of-two capacity so `idx & (CAP - 1)` wraps
/// without a branch.
const CAP: usize = 2;
const MASK: usize = CAP - 1;

struct SpscRing {
    /// Consumer read cursor (monotonic, wraps modulo `usize::MAX + 1`).
    head: AtomicUsize,
    /// Producer write cursor.
    tail: AtomicUsize,
    slots: [UnsafeCell<u32>; CAP],
}

// SAFETY: Loom's UnsafeCell enforces access discipline across threads; the
// SPSC discipline is that the producer writes `slots[tail % CAP]` before
// releasing `tail`, and the consumer reads `slots[head % CAP]` after
// acquiring `tail`.
unsafe impl Sync for SpscRing {}

impl SpscRing {
    fn new() -> Self {
        SpscRing {
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            slots: [UnsafeCell::new(0), UnsafeCell::new(0)],
        }
    }

    /// Producer: try to enqueue. Returns false if full.
    fn try_push(&self, v: u32) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        if tail.wrapping_sub(head) == CAP {
            return false;
        }
        // SAFETY: producer is the sole writer of slot tail % CAP; consumer
        // cannot observe it until we release-store `tail`.
        self.slots[tail & MASK].with_mut(|p| unsafe { *p = v });
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        true
    }

    /// Consumer: try to dequeue. Returns None if empty.
    fn try_pop(&self) -> Option<u32> {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        if head == tail {
            return None;
        }
        // SAFETY: consumer is the sole reader of slot head % CAP once the
        // producer has released `tail > head`.
        let v = self.slots[head & MASK].with(|p| unsafe { *p });
        self.head.store(head.wrapping_add(1), Ordering::Release);
        Some(v)
    }
}

/// Publication safety: producer writes one value, consumer runs once. If
/// the consumer observes a value at all, it must be the exact value the
/// producer published (no torn read across the release/acquire pair on
/// `tail`). Whichever element the consumer misses is drained afterwards,
/// and the ring must come back to empty.
#[test]
fn publication_release_acquire() {
    loom::model(|| {
        let ring = Arc::new(SpscRing::new());

        let r1 = Arc::clone(&ring);
        let p = thread::spawn(move || {
            let ok = r1.try_push(0xDEAD_BEEF);
            assert!(ok, "fresh ring has space for one element");
        });

        let r2 = Arc::clone(&ring);
        let c = thread::spawn(move || r2.try_pop());

        p.join().unwrap();
        let popped = c.join().unwrap();

        match popped {
            None => {
                assert_eq!(ring.try_pop(), Some(0xDEAD_BEEF));
            }
            Some(v) => assert_eq!(v, 0xDEAD_BEEF, "torn publish observed"),
        }
        assert!(ring.try_pop().is_none(), "ring must drain to empty");
    });
}

/// Capacity-respecting round trip: producer pushes exactly CAP values with
/// no retry, consumer attempts CAP pops with no retry. This avoids the
/// spin-loop permutation blow-up while still exercising the producer /
/// consumer race on concurrent index updates.
#[test]
fn spsc_capacity_round_trip() {
    loom::model(|| {
        let ring = Arc::new(SpscRing::new());

        let r1 = Arc::clone(&ring);
        let p = thread::spawn(move || {
            let a = r1.try_push(10);
            let b = r1.try_push(20);
            (a, b)
        });

        let r2 = Arc::clone(&ring);
        let c = thread::spawn(move || {
            let x = r2.try_pop();
            let y = r2.try_pop();
            (x, y)
        });

        let (pa, pb) = p.join().unwrap();
        let (cx, cy) = c.join().unwrap();

        assert!(pa && pb, "producer must fit 2 in capacity-2 ring");

        let mut seen = alloc::vec::Vec::<u32>::new();
        if let Some(v) = cx {
            seen.push(v);
        }
        if let Some(v) = cy {
            seen.push(v);
        }
        while let Some(v) = ring.try_pop() {
            seen.push(v);
        }

        // FIFO order must be preserved (RB3 + RB4 + RB7).
        assert_eq!(
            seen,
            alloc::vec![10u32, 20u32],
            "FIFO violation: {:?}",
            seen
        );
    });
}
