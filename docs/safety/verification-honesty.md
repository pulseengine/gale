# Verification Honesty Assessment

**Date:** 2026-03-22

## What Runs on Every Commit (CI-Enforced)

| Check | Tool | What It Proves |
|-------|------|---------------|
| Runtime tests | `cargo test` | Functions produce expected outputs, no panics (~995 tests) |
| Lint | `cargo clippy` | No common Rust pitfalls (pedantic + safety-critical lints) |
| Strip gate | `verus-strip gate` | plain/src/ is in sync with src/ (no stale stripped code) |
| Zephyr integration | west + QEMU | 36 upstream test suites pass with Gale replacing C code |
| Multi-arch | Renode | Semaphore tests pass on M4F, M33, R5 hardware emulation |
| Coverage | cargo-llvm-cov + gcov | Line coverage metrics uploaded to Codecov |

**These are functional tests, not formal proofs.**

## What Exists But Does NOT Run in CI

| Tool | What It Proves | How to Run | Last Verified |
|------|---------------|------------|---------------|
| Verus (Z3) | SMT verification of requires/ensures contracts | `bazel test //:verus_test` | Local only, no CI record |
| Rocq | Theorem proofs about abstract invariants | `bazel test //proofs:*` | Local only, no CI record |
| Lean 4 | Mathematical scheduling theory proofs | `bazel test //proofs/lean:*` | Local only, no CI record |
| Kani BMC | Bounded model checking (185 harnesses) | `bazel test //:kani_test` | Local only, no CI record |
| Miri | Undefined behavior detection | `bazel test //:miri_test` | Local only, no CI record |

**All formal verification requires Bazel + Nix toolchains.**

## The Gap

A commit could break any formal proof and CI would not catch it. The README claims "39/39 Verus verified" but this is based on local runs, not CI-enforced regression testing.

## What "Formally Verified" Actually Means for Gale Today

1. The Verus annotations EXIST in every src/*.rs file
2. They WERE verified to pass at some point (locally)
3. They are NOT regression-tested on every commit
4. The Verus-stripped code (plain/src/) IS tested functionally on every commit
5. The Zephyr integration IS tested on every commit

This is closer to "well-specified and comprehensively tested with local formal verification" than "continuously formally verified."

## Path to Real Formal Verification CI

Option A: Add Bazel CI workflow with Nix (complex setup, but runs everything)
Option B: Add a nightly/weekly cron job for formal verification (less frequent but catches drift)
Option C: Add individual verification tools to GitHub Actions without Bazel (simpler but more workflows)

The seL4 team spent a decade on their proofs. This is honest about where we are.
