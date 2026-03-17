//! Gale — formally verified Zephyr kernel primitives.
//!
//! Zephyr's wind, hardened through formal verification.
//! ASIL-D safety-critical, ISO 26262 certified kernel objects.
//!
//! Verification tracks:
//! - **Verus (this crate)**: SMT-backed proofs of functional correctness,
//!   memory safety, and absence of arithmetic overflow.
//! - **Rocq-of-Rust (plain/ directory)**: Theorem-prover-backed proofs of
//!   deeper properties (refinement, noninterference, deadlock freedom).
//!
//! ## Modules
//!
//! - [`error`] — Zephyr-compatible error codes
//! - [`priority`] — Bounded thread priority type
//! - [`thread`] — Thread state machine model
//! - [`wait_queue`] — Priority-ordered wait queue
//! - [`sem`] — Counting semaphore (port of kernel/sem.c)
//! - [`mutex`] — Reentrant mutex (port of kernel/mutex.c)
//! - [`condvar`] — Condition variable (port of kernel/condvar.c)

#![no_std]
#![allow(unused_imports)]
// Verus-verified code: arithmetic safety and index bounds are proven by the
// SMT solver (requires/ensures clauses). These clippy lints are redundant
// for formally verified code and would require #[allow] on every function.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    clippy::self_assignment,
    clippy::absurd_extreme_comparisons,
    clippy::wildcard_enum_match_arm,
    clippy::checked_conversions
)]

pub mod error;
pub mod priority;
pub mod thread;
pub mod wait_queue;
pub mod sem;
pub mod mutex;
pub mod condvar;
pub mod msgq;
pub mod pipe;
pub mod stack;
#[cfg(not(verus_keep_ghost))] pub mod fifo;
#[cfg(not(verus_keep_ghost))] pub mod lifo;
#[cfg(not(verus_keep_ghost))] pub mod timer;
#[cfg(not(verus_keep_ghost))] pub mod event;
#[cfg(not(verus_keep_ghost))] pub mod mem_slab;
#[cfg(not(verus_keep_ghost))] pub mod queue;
#[cfg(not(verus_keep_ghost))] pub mod futex;
#[cfg(not(verus_keep_ghost))] pub mod mbox;
#[cfg(not(verus_keep_ghost))] pub mod timeout;
#[cfg(not(verus_keep_ghost))] pub mod poll;
#[cfg(not(verus_keep_ghost))] pub mod sched;
#[cfg(not(verus_keep_ghost))] pub mod thread_lifecycle;
#[cfg(not(verus_keep_ghost))] pub mod timeslice;
#[cfg(not(verus_keep_ghost))] pub mod heap;
#[cfg(not(verus_keep_ghost))] pub mod kheap;
#[cfg(not(verus_keep_ghost))] pub mod work;
#[cfg(not(verus_keep_ghost))] pub mod fatal;
#[cfg(not(verus_keep_ghost))] pub mod fault_decode;
#[cfg(not(verus_keep_ghost))] pub mod mempool;
#[cfg(not(verus_keep_ghost))] pub mod dynamic;
#[cfg(not(verus_keep_ghost))] pub mod smp_state;
#[cfg(not(verus_keep_ghost))] pub mod stack_config;
#[cfg(not(verus_keep_ghost))] pub mod device_init;
#[cfg(not(verus_keep_ghost))] pub mod mem_domain;
#[cfg(not(verus_keep_ghost))] pub mod spinlock;
#[cfg(not(verus_keep_ghost))] pub mod atomic;
#[cfg(not(verus_keep_ghost))] pub mod userspace;
#[cfg(not(verus_keep_ghost))] pub mod ring_buf;
