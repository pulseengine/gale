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

pub mod error;
pub mod priority;
pub mod thread;
pub mod wait_queue;
pub mod sem;
pub mod mutex;
pub mod condvar;
