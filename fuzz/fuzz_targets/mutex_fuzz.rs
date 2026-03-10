//! Fuzz target: random operation sequences on the mutex.
//!
//! Run with: cargo fuzz run mutex_fuzz

#![no_main]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::wildcard_enum_match_arm,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
)]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use gale::error::*;
use gale::mutex::{Mutex, UnlockResult};
use gale::priority::Priority;
use gale::thread::Thread;

#[derive(Arbitrary, Debug)]
enum FuzzOp {
    TryLock { thread_id: u32 },
    Unlock { thread_id: u32 },
    LockBlocking { thread_id: u32, priority: u8 },
}

#[derive(Arbitrary, Debug)]
struct FuzzInput {
    ops: Vec<FuzzOp>,
}

fuzz_target!(|input: FuzzInput| {
    let mut m = Mutex::init();

    for op in &input.ops {
        match op {
            FuzzOp::TryLock { thread_id } => {
                m.try_lock(*thread_id);
            }
            FuzzOp::Unlock { thread_id } => {
                let _ = m.unlock(*thread_id);
            }
            FuzzOp::LockBlocking { thread_id, priority } => {
                let prio = (*priority as u32) % 32;
                if m.is_locked() && m.owner_get() != Some(*thread_id) && m.num_waiters() < 60 {
                    if let Some(p) = Priority::new(prio) {
                        let mut t = Thread::new(*thread_id, p);
                        t.dispatch();
                        m.lock_blocking(t);
                    }
                }
            }
        }

        // Invariant M1 — any violation here is a bug
        let lc = m.lock_count_get();
        let owner = m.owner_get();
        assert!(
            (lc > 0) == owner.is_some(),
            "M1 VIOLATION: lock_count={lc}, owner={owner:?}"
        );

        // Invariant M2 — waiters imply locked
        if m.num_waiters() > 0 {
            assert!(
                m.is_locked(),
                "M2 VIOLATION: waiters={} but mutex unlocked",
                m.num_waiters()
            );
        }
    }
});
