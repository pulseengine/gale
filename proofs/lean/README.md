# Lean 4 proofs — and the gate that checks them

Seven files, 136 theorems/lemmas, **0 `sorry` / 0 `axiom`**. Every file has a
`lean_proof_test` target in `BUILD.bazel`, and the `lean` job in
`.github/workflows/formal-verification.yml` runs all seven on every PR.

    //proofs/lean:scheduling_test        Rate Monotonic Analysis (Liu & Layland 1973)
    //proofs/lean:priority_ceiling_test  Priority Ceiling Protocol
    //proofs/lean:priority_queue_test    priority queue invariants (sorted-list model)
    //proofs/lean:mpu_region_test        ARM MPU region validation
    //proofs/lean:systick_test           ARM SysTick timer arithmetic
    //proofs/lean:ring_buf_test          ring buffer index arithmetic
    //proofs/lean:atomic_test            wrapping atomic arithmetic

Run them locally with `bazel test //proofs/lean:all`.

## Why the job exists (gale#286)

Those targets existed from the day the files landed and **no workflow invoked
them**. `formal-verification.yml` ran `verus`, `kani`, `rocq` and stopped, while
`bazel-tests.yml` carried a comment claiming that workflow ran "the Rocq/Lean
targets" — true for Rocq, false for Lean.

So "0 `sorry` across 136 theorems" was a **grep result, not a verification
result**. Nothing would have reported it if the proofs stopped closing, or
stopped compiling against Lean 4.27.0 and current mathlib.

## Cost — measured, because it is the only reason to hesitate

| | time |
|---|---|
| all 7 tests, warm mathlib | **8.7 s** (critical path 6.6 s) |
| cold, from an empty bazel cache | 258 s |

The cold figure is almost entirely a one-time download of **7869 prebuilt mathlib
files** from the upstream Azure cache; elaboration critical path is 13.9 s.
**mathlib is fetched, not built** — `patches/rules_lean_mathlib_timeout.patch`
makes it look like an hours-long compile, and it is not one. Cheap enough to gate
every PR rather than run on a schedule.

## The gate bites — verified, not assumed

A gate that has never been shown to fail is not a gate. Negative control:

```lean
theorem deliberately_false_control (n : Nat) : n < n := by omega
```

appended to `Scheduling.lean` gives

    //proofs/lean:scheduling_test   FAILED TO BUILD   (exit 1)

`omega` reports the counterexample rather than closing the goal, so the failure
is an elaboration error, not a test assertion. Restore the file afterwards.

## A trap this workflow's path filters used to contain

`formal-verification.yml` filters on `proofs/**`, `src/**`, `plain/**`, `ffi/**`,
`tests/**` — and, until #288, **not on itself**. A PR editing only the workflow
therefore did not trigger it, so a change to the gate could merge without the
gate ever running. #288 hit this: it added the `lean` job and, on its first push,
`formal-verification.yml` produced **zero runs** on the branch.

The workflow now lists its own path (and `BUILD.bazel` / `MODULE.bazel`, which
were on the `push` filter but missing from `pull_request`). If you add a job
here, confirm it actually ran on your PR before merging — a green PR that
silently skipped the workflow is the failure mode this paragraph exists to
prevent.

## Scope

These are **mathematical** proofs about scheduling and arithmetic, not refinement
proofs against gale's Rust. `Scheduling.lean` proves the Liu & Layland
utilisation bound and rate-monotonic optimality; it does **not** prove anything
about `src/executor.rs`. The jittered/supply-derived WCRT recurrence that track
T3 (`REQ-OS-SCHED-001`) needs lives in spar, not here — see #287.
