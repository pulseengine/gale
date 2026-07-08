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

---

## Update 2026-07-08 — claims ledger (prompted by external formal-methods review)

A formal-verification researcher raised critical views of our "formal proof" claims.
That is the external check we otherwise lack, and it is correct on the substance. This
section states, per claim, exactly what is established, at what level, and under what
trusted base — so every claim we make is defensible, and the headline framing is
reconciled with the honest internal picture above.

### The overarching distinction we must always make: SOURCE vs SHIPPED

Every proof/check below establishes a property of a **source** artifact — the
Verus-annotated Rust, or the wasm compiled from it. The **shipped** artifact for the
gust/dissolve line is *native code produced by `meld → loom → synth`*, a young,
fast-moving toolchain (the "vibe-coded interpreter/compiler" an external reviewer
correctly flagged). There is **no translation-validation and no equivalence proof**
that the dissolve preserves the proven semantics. The shipped binary's correctness
therefore rests on three things, only the first of which is a proof:

1. source-level proofs/checks (Verus/Rocq/Kani — on the Rust/wasm, see below);
2. **differential testing** — `wasmtime` (reference semantics) vs the dissolved native,
   over sampled or finite input ranges (a *test*, not a proof);
3. **trust in the dissolve toolchain** (synth/loom/meld), which is not itself verified.

Honest one-liner: **"source-level formally checked; the shipped dissolved artifact is
differentially tested against a reference semantics, not proven equivalent."** Any
claim of an "end-to-end formally verified pipeline" or "one formally-verified artifact"
across the wasm→native boundary is **not currently supported** and must not be made.

### Rocq: real proofs vs shipped stubs

`proofs/*.v` is **not uniformly proven**. Ground truth (counts of `Qed.` vs `Admitted.`):

- **9 files fully proven (0 `Admitted`):** sem (82 `Qed`), pipe (10), stack (10),
  mutex (9), msgq (9), condvar (8), event (7), mem_slab (7), timer (7). These are the
  "9 abstract invariant proofs" the README counts, and the count is accurate.
- **3 files are 100% `Admitted` stubs (0 `Qed`):** `poll_proofs.v` (22), `sched_proofs.v`
  (23), `thread_lifecycle_proofs.v` (29) = **74 admitted theorems** shipped in the same
  directory. Several carry `(* … Coq 9.0 tactic … *)` notes — they are proof *scaffolding*,
  not proofs. A reader browsing `proofs/` will reasonably read these as "Rocq proofs";
  they are not. **These must be labelled WIP/stub, and the "Rocq" claim must say "9 of 12
  invariant modules proven; poll/sched/thread_lifecycle are admitted stubs."**
- **All Rocq is over hand-written Z-valued models, NOT connected to the Rust** (README
  line 124 already says this). Rocq here is *design-level* invariant checking, not an
  implementation proof. Correct framing: "abstract-model theorem proving for design
  validation," never "the implementation is Rocq-proven."

### Kani: which harnesses are exhaustive vs bounded

Kani is bounded model checking. A harness over `kani::any::<uN>()` with **no loop** is
*exhaustive over the full type domain* (a genuine proof of the property for all inputs).
A harness that **`kani::assume`s a bound** (`assume(cap <= 16)`) or contains a loop with an
unwind bound proves the property **only within that bound** — and the bound's sufficiency
is not itself argued.

- The kernel harnesses in `tests/kani_harnesses.rs` / `kani_equivalence.rs` are largely
  the **bounded** kind (`assume(count<=20)`, `assume(capacity<=100)`, `assume(msg_size<=256)`).
  Defensible statement: "bounded-checked up to N," **not** "proven for all inputs."
- The gust **driver** decision cores are the **exhaustive** kind and are the stronger
  claim: `usart_rx_decide` over all 2³² SR (uart-thin), and gpio-thin's 4 harnesses are
  straight-line over `kani::any()` (pin-config total/injective/mode-consistent, slot
  in-range over all `u32`, unknown-mode-safe) — genuinely complete **for the source
  decision function**. dma-own's 6 harnesses cover a small-bounded ring (RING=4); state
  "proven for the modelled ring size," not unbounded. In all cases the proof is of the
  **source** logic; the dissolved object is differentially checked, not proven (see above).

### gust / dissolve perf-and-fact claims — precise status

- **"0.45× proof-carrying floor" (`gust_floor_bench`)** — is an **exhaustive differential
  test over the finite range [524,1524]** (1001 inputs): `mix_proven ≡ mix_native ≡ gust_mix`,
  bit-for-bit. That is a *proof of equivalence for those 1001 inputs only* — sound for the
  range, silent outside it. It does **not** verify the dissolve toolchain and does not
  prove the specialization sound in general. Correct wording: "exhaustive differential
  check over the proven range," not "soundness gate." The *0.45×* is a measured
  qemu-`-icount` ratio, real; the "proof-carrying" adjective describes synth's design
  intent (synth#494 ordeal/LRAT), which is **synth's** claim to substantiate, not ours.
- **"wsc.facts phase-1 verified" (`wsc-facts-phase1.sh`)** — is a **byte-identity
  regression test** (facts-carrying vs stripped compile) plus a fail-safe-warning check.
  It establishes that phase-1 ingestion changes no bytes; it proves nothing about phase-2
  consumption or about correctness. The script's own header says "regression test /
  tripwire" — keep that framing; do not upgrade it to "verified."
- **"the same SQ/CQ model, proof-carrying" (RTIO note, `FIND-DRV-RTIO-001`)** — the
  `own<buffer>` ownership property is Kani-proven **at the source FSM level** (dma-own) and
  is a Component-Model *type* property of the WIT; it is a defensible *source/design* claim.
  It is **not** a claim that the shipped native enforces ownership at runtime (there is no
  runtime; the guarantee is compile-time/type-level and inherits the source-vs-shipped gap).

### Reconciling the headline

`README.md:15` ("Formally verified Rust replacement … triple-track verification") and the
architecture doc's "one formally-verified artifact" read as stronger than the honest
internal picture in this file. They should be brought into line (a positioning decision):
lead with what is *actually strong* — comprehensive functional + differential testing,
real Verus SMT on the source (with the 133 `external_body` trust units named), 9 real Rocq
model proofs — and state the two limits up front: **(a)** formal tools do not run in CI
(local only), and **(b)** the shipped dissolved artifact is tested against a reference
semantics, not proven equivalent to the verified source. Nothing here weakens the
engineering; it makes each claim one a skeptic cannot dismantle.
