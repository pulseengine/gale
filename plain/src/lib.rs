//! Gale — plain Rust implementation of Zephyr kernel primitives.
//!
//! This crate mirrors the Verus-annotated code in `../src/` but without
//! verification annotations. It serves three purposes:
//!
//! 1. Source for Rocq-of-Rust translation (to .v files for theorem proving)
//! 2. Target for standard Rust testing tools (miri, kani, fuzz, etc.)
//! 3. Reference implementation for traceability to Zephyr kernel/sem.c
//!
//! The logic is identical to the Verus code — any divergence is a bug.

#![no_std]
#![deny(unsafe_code)]

pub mod error;
pub mod priority;
pub mod mutex;
pub mod sem;
pub mod condvar;
pub mod msgq;
pub mod pipe;
pub mod stack;
pub mod thread;
pub mod wait_queue;
