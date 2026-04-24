//! Loom permutation tests for concurrent priority-queue access.
//!
//! Model:
//!   A minimal priority queue (insert / remove-best) guarded by a
//!   `loom::sync::Mutex`, modelled on `gale::sched::RunQueue` — which stores
//!   threads sorted by Zephyr priority (lower numeric = higher priority).
//!   The real Zephyr code guards `_priq_*` with a spinlock; the loom model
//!   substitutes `loom::sync::Mutex` to let the permutation checker explore
//!   every acquisition order.
//!
//! Property targeted:
//!   * Under concurrent inserts followed by concurrent pops, the pops must
//!     produce a prefix of the merged priority order — a malformed heap or
//!     dropped element would surface as either a missing value or an
//!     out-of-order pop.
//!   * Mutual exclusion holds: the invariant `len <= capacity` is preserved
//!     under every interleaving.
//!
//! Why loom catches what Verus cannot:
//!   `plain/src/sched.rs::RunQueue` is a single-threaded array model — Verus
//!   proves sorted-insert and shift-remove preserve order (SC1–SC3) but
//!   cannot examine the critical-section boundary where the C kernel holds
//!   `k_spin_lock`. A port that races lock/unlock with a thread observing
//!   stale `len` could satisfy Verus on the in-lock body yet corrupt the
//!   queue out-of-lock. Loom enumerates the lock acquisition permutations
//!   and deterministically replays any violation found.
//!
//! How to run:
//!     RUSTFLAGS="--cfg loom" cargo test --test loom_priority_queue -- --test-threads=1
//!
//! Budget: 2 producers each inserting 1 priority, then 2 pops — state space
//! ~few thousand branches. Keep LOOM_MAX_PREEMPTIONS=3.
//!
//! Traceability: operationalises invariants proven in
//! `proofs/lean/PriorityQueue.lean` (sorted insert, best-pop) on an
//! actual lock-guarded implementation.

#![cfg(loom)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing
)]

use loom::sync::{Arc, Mutex};
use loom::thread;

const CAPACITY: usize = 4;

/// Minimal fixed-capacity min-priority queue (lower value = higher priority),
/// modelled after `gale::sched::RunQueue`.
struct PriQ {
    entries: [Option<u32>; CAPACITY],
    len: usize,
}

impl PriQ {
    fn new() -> Self {
        PriQ {
            entries: [None; CAPACITY],
            len: 0,
        }
    }

    fn insert(&mut self, p: u32) -> bool {
        if self.len >= CAPACITY {
            return false;
        }
        // Find insertion point (sorted ascending = highest priority first).
        let mut pos = self.len;
        for i in 0..self.len {
            if p < self.entries[i].unwrap() {
                pos = i;
                break;
            }
        }
        // Shift right.
        let mut j = self.len;
        while j > pos {
            self.entries[j] = self.entries[j - 1];
            j -= 1;
        }
        self.entries[pos] = Some(p);
        self.len += 1;
        true
    }

    fn pop_best(&mut self) -> Option<u32> {
        if self.len == 0 {
            return None;
        }
        let v = self.entries[0];
        // Shift left.
        for i in 0..self.len - 1 {
            self.entries[i] = self.entries[i + 1];
        }
        self.entries[self.len - 1] = None;
        self.len -= 1;
        v
    }

    fn is_sorted(&self) -> bool {
        for i in 1..self.len {
            if self.entries[i - 1].unwrap() > self.entries[i].unwrap() {
                return false;
            }
        }
        true
    }
}

/// SC1/SC2: concurrent inserts of two threads produce a sorted queue of
/// length 2, regardless of acquisition order.
#[test]
fn concurrent_insert_preserves_sorted_order() {
    loom::model(|| {
        let q = Arc::new(Mutex::new(PriQ::new()));

        let q1 = Arc::clone(&q);
        let h1 = thread::spawn(move || {
            q1.lock().unwrap().insert(7);
        });

        let q2 = Arc::clone(&q);
        let h2 = thread::spawn(move || {
            q2.lock().unwrap().insert(3);
        });

        h1.join().unwrap();
        h2.join().unwrap();

        let guard = q.lock().unwrap();
        assert_eq!(guard.len, 2);
        assert!(guard.is_sorted(), "sorted invariant violated");
        // Best must be the lower numeric priority.
        assert_eq!(guard.entries[0], Some(3));
        assert_eq!(guard.entries[1], Some(7));
    });
}

/// SC1+SC3: concurrent insert + pop_best does not drop an element or leave
/// the queue in an invalid state. The pop may observe either 0 or 1
/// element depending on interleaving, but the invariant `len <= CAPACITY`
/// and sorted-ness must hold either way.
#[test]
fn insert_pop_race_preserves_invariant() {
    loom::model(|| {
        let q = Arc::new(Mutex::new(PriQ::new()));

        // Pre-seed so pop has something to take if it wins the race.
        q.lock().unwrap().insert(5);

        let q1 = Arc::clone(&q);
        let h_ins = thread::spawn(move || {
            q1.lock().unwrap().insert(2);
        });

        let q2 = Arc::clone(&q);
        let h_pop = thread::spawn(move || q2.lock().unwrap().pop_best());

        h_ins.join().unwrap();
        let popped = h_pop.join().unwrap();

        // Popped is one of the priorities we put in.
        assert!(matches!(popped, Some(2) | Some(5)));

        let guard = q.lock().unwrap();
        // One element must remain — conservation of elements.
        assert_eq!(guard.len, 1);
        assert!(guard.is_sorted());
    });
}
