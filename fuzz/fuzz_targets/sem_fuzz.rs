//! Fuzz target: random operation sequences on the semaphore.
//!
//! Run with: cargo fuzz run sem_fuzz

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
use gale::priority::Priority;
use gale::sem::Semaphore;
use gale::thread::Thread;

#[derive(Arbitrary, Debug)]
enum FuzzOp {
    Give,
    TryTake,
    TakeBlocking { thread_id: u32, priority: u8 },
    Reset,
}

#[derive(Arbitrary, Debug)]
struct FuzzInput {
    initial_count: u32,
    limit: u32,
    ops: Vec<FuzzOp>,
}

fuzz_target!(|input: FuzzInput| {
    // Sanitize inputs
    let limit = if input.limit == 0 { 1 } else { input.limit.min(10_000) };
    let initial_count = input.initial_count.min(limit);

    let mut sem = match Semaphore::init(initial_count, limit) {
        Ok(s) => s,
        Err(_) => return,
    };

    for op in &input.ops {
        match op {
            FuzzOp::Give => {
                sem.give();
            }
            FuzzOp::TryTake => {
                sem.try_take();
            }
            FuzzOp::TakeBlocking { thread_id, priority } => {
                let prio = (*priority as u32) % 32;
                if sem.count_get() == 0 {
                    if let Some(p) = Priority::new(prio) {
                        let mut t = Thread::new(*thread_id, p);
                        t.dispatch();
                        sem.take_blocking(t);
                    }
                }
            }
            FuzzOp::Reset => {
                sem.reset();
            }
        }

        // Invariant check — any violation here is a bug
        assert!(
            sem.count_get() <= sem.limit_get(),
            "INVARIANT VIOLATION: count {} > limit {}",
            sem.count_get(),
            sem.limit_get()
        );
    }
});
