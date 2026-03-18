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
#![allow(unused_imports, unused_variables)]
// Verus-stripped code: the stripping process produces patterns that trigger
// clippy lints (e.g., `x = x | y` instead of `x |= y`, `x as u64` casts).
// These are suppressed since this is generated output, not hand-written code.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    clippy::self_assignment,
    clippy::absurd_extreme_comparisons,
    clippy::wildcard_enum_match_arm,
    clippy::checked_conversions,
    clippy::assign_op_pattern,
    clippy::cast_lossless,
    clippy::wildcard_imports,
    clippy::match_same_arms,
    clippy::option_if_let_else,
    clippy::if_not_else,
    clippy::new_without_default,
    clippy::must_use_candidate,
    clippy::unused_self,
    clippy::needless_bool,
    clippy::useless_asref,
    clippy::redundant_field_names,
    clippy::manual_map,
    clippy::single_match,
    clippy::return_self_not_must_use,
    clippy::len_without_is_empty,
    clippy::match_like_matches_macro,
    clippy::manual_is_power_of_two,
    clippy::manual_div_ceil,
    clippy::too_long_first_doc_paragraph,
    clippy::len_zero,
    clippy::single_match_else
)]
#[cfg(kani)]
extern crate kani;
pub mod atomic;
pub mod condvar;
pub mod device_init;
pub mod dynamic;
pub mod error;
pub mod event;
pub mod fatal;
pub mod fault_decode;
pub mod fifo;
pub mod futex;
pub mod heap;
pub mod kheap;
pub mod lifo;
pub mod mbox;
pub mod mem_domain;
pub mod mem_slab;
pub mod mempool;
pub mod msgq;
pub mod mutex;
pub mod pipe;
pub mod poll;
pub mod priority;
pub mod queue;
pub mod ring_buf;
pub mod sched;
pub mod sem;
pub mod smp_state;
pub mod spinlock;
pub mod stack;
pub mod stack_config;
pub mod thread;
pub mod thread_lifecycle;
pub mod timeout;
pub mod timer;
pub mod timeslice;
pub mod userspace;
pub mod wait_queue;
pub mod work;
