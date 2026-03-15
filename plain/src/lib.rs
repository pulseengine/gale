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
#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    clippy::self_assignment,
    clippy::absurd_extreme_comparisons
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
pub mod fifo;
pub mod lifo;
pub mod timer;
pub mod event;
pub mod mem_slab;
pub mod queue;
pub mod futex;
pub mod mbox;
pub mod timeout;
pub mod poll;
