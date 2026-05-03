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

## Trusted Code: `external_body`, `assume_specification`, and `--no-cheating`

Verus is a partial-correctness checker: it proves what you ask it to prove, given the *trusted base* you declare. Two annotations widen that trusted base, and an honest claim of "verified" must enumerate them:

- **`#[verifier::external_body]`** — Verus does not look inside the function body; it trusts the declared `requires`/`ensures` contract verbatim. Used for FFI shims, raw-pointer arithmetic, and a handful of intrinsics where we model the Zephyr C contract instead of re-proving it.
- **`assume_specification`** — Verus assumes a specification holds for an *external* function (typically from `core` or `vstd`) without verifying it against the function's body. Used very sparingly.

Running `bazel test //:verus_test` succeeds when the bodies Verus *did* check pass; it does **not** report whether `external_body` and `assume_specification` annotations are sound. The `verus --no-cheating` flag refuses to verify any module that contains either annotation, which is the correct setting for a maximalist soundness claim. Today's gale CI does not pass `--no-cheating` because we accept these trust units; a `--no-cheating` run would correctly fail.

### Trusted-base inventory

133 `#[verifier::external_body]` instances and 2 `assume_specification` calls across the verified surface, concentrated in modules that bridge to C peripherals or low-level memory:

| Module | `external_body` count | What's trusted |
|---|---:|---|
| `src/net_buf.rs` | 36 | Network buffer FFI: pointer offsets, raw byte access, `core::ptr` operations |
| `src/pm.rs` | 22 | Power-management state transitions (hardware registers we don't model) |
| `src/ipc.rs` | 22 | Inter-process-communication shims (cross-CPU notifications) |
| `src/mmu.rs` | 20 | Page-table arithmetic and TLB ops |
| `src/usage.rs` | 18 | CPU-usage statistics counters (atomic increments on shared state) |
| `src/thread_lifecycle.rs` | 5 | Thread teardown sequences |
| `src/sched.rs` | 4 | Scheduler entry points called from C |
| `src/poll.rs` | 3 | `assume_specification` for `core::cell` projections |
| `src/mpu.rs` | 3 | MPU region setup |
| **total** | **133** | + 2 `assume_specification` |

### What this means for the headline claim

The "39/39 modules, 805 verified, 0 errors" claim refers to what Verus *checked*. It does NOT mean every line in every module was checked. The 133 `external_body` annotations are the unchecked surface; their correctness is justified by the matching Zephyr C source they shim, not by SMT.

A reader citing the bench numbers should know:
- The verified bodies are SMT-checked.
- The `external_body` shims are reviewed-by-eye against the C they bridge to, with FFI-contract docstrings.
- `verus --no-cheating` would intentionally reject these and is not the gate we run.
