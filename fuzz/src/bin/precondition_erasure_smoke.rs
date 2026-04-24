//! Deterministic sweep over the FFI precondition-erasure case table.
//!
//! This is the smoke driver for the fuzz harness at
//! `fuzz_targets/precondition_erasure.rs`. It does NOT require nightly
//! or cargo-fuzz; it just runs every case on a fixed boundary set plus
//! 256 pseudo-random inputs per case and prints a coverage table.
//!
//! Run:
//!     cargo run -p gale-fuzz --bin precondition_erasure_smoke

fn main() {
    let summary = gale_fuzz::precondition_erasure::deterministic_sweep();
    summary.print();
    if summary.any_issues() {
        // Non-zero exit so CI can wire this up later. Existing UCAs
        // (U-1 overflow, U-8 tid aliasing, U-9 CPU_MASK, U-11 overflow,
        // U-12 saturation) all live in the violation region, so they
        // register as coverage, not as crashes. A non-zero exit here
        // means the harness found something genuinely new: a
        // postcondition mismatch in the valid region, or a panic on
        // valid input.
        std::process::exit(1);
    }
}
