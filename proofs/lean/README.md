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

Run them locally with `bazel test //proofs/lean:all` — which is also exactly what
CI runs. **Do not go back to an enumerated target list.** The list form silently
failed to cover a new proof the first time one was added: `PartitionSupply.lean`
made it 8 targets while the workflow still named 7, and the job went green
without elaborating it. That is gale#286's shape — a gate that exists but does
not run — one level down. With `:all`, a new `lean_proof_test` is gated by
construction rather than by remembering to edit a workflow.

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

## Why the CI job passes `--lockfile_mode=off`

`rules_lean`'s `mathlib_repo` takes `host_platform` as a repo-rule **attribute**,
so it is written into `MODULE.bazel.lock` and frozen to whichever OS generated
the lock. Ours says:

    "mathlib": { "attributes": { "host_platform": "darwin_aarch64", … } }

A Linux runner reusing that lock builds `_lean_toolchain` from the **macOS**
toolchain, and `sh` ends up parsing a Mach-O binary:

    lake: 1: …!H__PAGEZERO?__TEXT@@__text__TEXTXdX?__stubs__TEXT?: not found
    lake: 9: Syntax error: Unterminated quoted string

Which is why the proofs passed on a developer Mac and could not run in CI at all.
`--lockfile_mode=off` makes bazel re-resolve the extension for the actual host.

Filed upstream as **rules_lean#30** — a repo rule capturing the host platform as
an attribute makes a checked-in lockfile non-portable, which defeats the purpose
of committing one. Remove this flag once the platform is resolved at fetch time.

**It weakens lockfile verification for this step only, and it cannot weaken a
proof.** The theorems still elaborate under a real Lean kernel, or the job is red.
