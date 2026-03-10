//! Fuzz target: random operation sequences on the condition variable.
//!
//! Run with: cargo fuzz run condvar_fuzz

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
use gale::condvar::CondVar;
use gale::priority::Priority;
use gale::thread::Thread;

#[derive(Arbitrary, Debug)]
enum FuzzOp {
    Signal,
    Broadcast,
    WaitBlocking { thread_id: u32, priority: u8 },
}

#[derive(Arbitrary, Debug)]
struct FuzzInput {
    ops: Vec<FuzzOp>,
}

fuzz_target!(|input: FuzzInput| {
    let mut cv = CondVar::init();

    for op in &input.ops {
        match op {
            FuzzOp::Signal => {
                cv.signal();
            }
            FuzzOp::Broadcast => {
                cv.broadcast();
            }
            FuzzOp::WaitBlocking { thread_id, priority } => {
                let prio = (*priority as u32) % 32;
                if cv.num_waiters() < 60 {
                    if let Some(p) = Priority::new(prio) {
                        let mut t = Thread::new(*thread_id, p);
                        t.dispatch();
                        cv.wait_blocking(t);
                    }
                }
            }
        }

        // Invariant: waiters bounded
        assert!(
            cv.num_waiters() <= 64,
            "INVARIANT VIOLATION: num_waiters={} > 64",
            cv.num_waiters()
        );
    }
});
