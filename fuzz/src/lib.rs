//! Shared harness for the FFI precondition-erasure fuzzer.
//!
//! This library is reused by:
//!   * fuzz_targets/precondition_erasure.rs  — libfuzzer entry point
//!   * src/bin/precondition_erasure_smoke.rs — deterministic sweep
//!
//! See `precondition_erasure` module for the case table and runner.

pub mod precondition_erasure;
