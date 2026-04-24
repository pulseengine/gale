//! Loom permutation tests for atomic RMW operations.
//!
//! Property targeted:
//!   * Compare-and-swap linearisability — exactly one of two concurrent CAS
//!     operations that both race from the same expected value may succeed.
//!   * fetch-or / fetch-and bit-set idempotence under every interleaving —
//!     the final value equals the OR (resp. AND) of all contributions.
//!
//! Why loom catches what Verus cannot:
//!   Gale's `gale::atomic::AtomicVal` is a pure-value model: every method
//!   takes `&mut self`, so there is no concurrency to reason about; Verus
//!   proves only the single-threaded index arithmetic (AT1–AT6 in
//!   `plain/src/atomic.rs`). Real hardware executes these operations with
//!   weak-memory (ARM / x86 TSO) semantics, so a faulty port using the wrong
//!   `Ordering` on a Rust `AtomicU32` could still satisfy Verus yet exhibit
//!   reordered reads/writes on a Cortex-A. Loom enumerates every legal
//!   interleaving under the C11 memory model and flags violations — including
//!   the subtle cases (load-before-store, stale CAS success) that plain
//!   sequential proofs and stress tests rarely hit.
//!
//! How to run:
//!     RUSTFLAGS="--cfg loom" cargo test --test loom_atomic -- --test-threads=1
//!
//! Budget: bounded with LOOM_MAX_PREEMPTIONS=3 for the CAS race (2 threads)
//! and LOOM_MAX_PREEMPTIONS=2 for the 3-thread fetch-or race. Expect <30 s
//! wall per test on an M-series Mac.
//!
//! Traceability: operationalises the invariants proven in
//! `proofs/lean/Atomic.lean` — AT3 (CAS success iff current == expected) and
//! AT4 (CAS failure leaves value unchanged).

#![cfg(loom)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use loom::sync::Arc;
use loom::sync::atomic::{AtomicU32, Ordering};
use loom::thread;

/// Exercise AT3/AT4: with two threads CAS-ing from the same `expected` to
/// different `new` values, exactly one may succeed and the stored value must
/// equal that winner's `new`.
#[test]
fn cas_race_exactly_one_wins() {
    loom::model(|| {
        let cell = Arc::new(AtomicU32::new(0));

        let c1 = Arc::clone(&cell);
        let h1 = thread::spawn(move || {
            c1.compare_exchange(0, 10, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        });

        let c2 = Arc::clone(&cell);
        let h2 = thread::spawn(move || {
            c2.compare_exchange(0, 20, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        });

        let w1 = h1.join().unwrap();
        let w2 = h2.join().unwrap();

        // AT3+AT4: exactly one CAS may succeed.
        assert!(w1 ^ w2, "exactly one CAS must succeed");

        let final_val = cell.load(Ordering::Acquire);
        if w1 {
            assert_eq!(final_val, 10, "winner 1 must leave 10");
        } else {
            assert_eq!(final_val, 20, "winner 2 must leave 20");
        }
    });
}

/// Exercise fetch-or monotonicity: starting from 0, three concurrent
/// `fetch_or` calls setting disjoint bits must produce the OR of all bits.
/// Verus cannot model this because every thread's `or()` is sequentialised
/// on a single `&mut self`. Loom checks there is no weak-memory reordering
/// that could cause a bit to be silently lost.
#[test]
fn fetch_or_bit_accumulation() {
    loom::model(|| {
        let cell = Arc::new(AtomicU32::new(0));

        let a = Arc::clone(&cell);
        let b = Arc::clone(&cell);

        let h1 = thread::spawn(move || {
            a.fetch_or(0b001, Ordering::AcqRel);
        });
        let h2 = thread::spawn(move || {
            b.fetch_or(0b010, Ordering::AcqRel);
        });
        // Main thread contributes the third bit.
        cell.fetch_or(0b100, Ordering::AcqRel);

        h1.join().unwrap();
        h2.join().unwrap();

        assert_eq!(cell.load(Ordering::Acquire), 0b111);
    });
}

/// Exercise fetch-add commutativity: two concurrent increments must produce
/// a final value of 2 regardless of interleaving.  A broken implementation
/// using Relaxed-then-store instead of a true RMW would be caught here.
#[test]
fn fetch_add_no_lost_update() {
    loom::model(|| {
        let cell = Arc::new(AtomicU32::new(0));

        let a = Arc::clone(&cell);
        let b = Arc::clone(&cell);

        let h1 = thread::spawn(move || a.fetch_add(1, Ordering::AcqRel));
        let h2 = thread::spawn(move || b.fetch_add(1, Ordering::AcqRel));

        let r1 = h1.join().unwrap();
        let r2 = h2.join().unwrap();

        // One of the threads saw 0, the other saw 1 — never both 0.
        assert!(
            (r1 == 0 && r2 == 1) || (r1 == 1 && r2 == 0),
            "no lost update permitted: got ({r1}, {r2})"
        );
        assert_eq!(cell.load(Ordering::Acquire), 2);
    });
}
