//! Fuzz target: FFI precondition-erasure bug class.
//!
//! Verified Rust decision functions carry Verus `requires` preconditions
//! that the C caller must uphold but *cannot be automatically checked*
//! at the FFI boundary. This harness feeds inputs spanning both the
//! valid-precondition region and the violation region to every primitive
//! covered by UCAs U-1 / U-5 / U-6 / U-8 / U-9 / U-10 / U-11 / U-12 / U-13.
//!
//! For each call the harness:
//!   1. Records whether the input satisfies the Verus `requires` clause.
//!   2. Invokes the plain (non-Verus) decision function under
//!      `std::panic::catch_unwind` — panics here in `overflow-checks = on`
//!      dev builds signal that precondition erasure has caused UB or
//!      arithmetic wrap the proof assumed away.
//!   3. Re-derives the expected `ensures` postcondition on valid inputs
//!      and flags any divergence as a verification/impl mismatch.
//!   4. Emits a structured line per failure so libfuzzer/AFL-style crash
//!      minimization can locate the offending `(case, args)` pair.
//!
//! The same logic is shared between the libfuzzer-sys entry point and the
//! deterministic `precondition_erasure_smoke` binary (see `fuzz/src/`).
//!
//! Run as libfuzzer:
//!     cargo +nightly fuzz run precondition_erasure -- -max_total_time=10
//!
//! Run the deterministic sweep (no nightly required):
//!     cargo run -p gale-fuzz --bin precondition_erasure_smoke

#![no_main]
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::arithmetic_side_effects,
)]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

use gale_fuzz::precondition_erasure::{run_case, Args, CASES};

#[derive(Arbitrary, Debug)]
struct FuzzInput {
    /// Which precondition case to exercise (modulo CASES.len()).
    case_idx: u8,
    a: u32,
    b: u32,
    c: u32,
    d: u32,
    flags: u8,
}

fuzz_target!(|input: FuzzInput| {
    let case = &CASES[(input.case_idx as usize) % CASES.len()];
    let args = Args {
        a: input.a,
        b: input.b,
        c: input.c,
        d: input.d,
        flag0: (input.flags & 0b0001) != 0,
        flag1: (input.flags & 0b0010) != 0,
        flag2: (input.flags & 0b0100) != 0,
        flag3: (input.flags & 0b1000) != 0,
    };
    // Swallow the outcome — libfuzzer treats panics as crashes, and
    // run_case panics on a genuine divergence. Report-only divergences
    // (postcondition mismatch under valid input) are returned as Ok and
    // surfaced via eprintln so they still stand out in libfuzzer logs.
    let _ = run_case(case, &args);
});
