# Wave report — Rocq no-lost-wakeups (T1 / v0.8.0 safety line)

Date: 2026-07-16
Branch: `feat/gust-rocq-no-lost-wakeups`
Requirement tag: [REQ-OS-TRITRACK-001]

## STEP 1 — Infrastructure found (the Rocq track was NOT empty repo-wide, but WAS empty for the executor)

- `proofs/*.v`: 13 pre-existing Rocq files, built by Bazel via `rules_rocq_rust`
  (`rocq_library` / `rocq_proof_test` in `proofs/BUILD.bazel`), toolchain resolved
  through Nix (`rules_nixpkgs_core`, nixpkgs pin in `MODULE.bazel`). Compiler:
  **The Rocq Prover, version 9.0.1** (no system `coqc`/`rocq` — the repo toolchain
  provides it; validated locally by running `bazel test //proofs:sched_proofs_test`
  → PASSED, 130 s cold).
- CI: `.github/workflows/formal-verification.yml` job `rocq` runs every
  `//proofs:*_proofs_test` target (push + PR + weekly cron).
- Ground truth of the pre-existing 13 files (matches
  `docs/safety/verification-honesty.md`): 9 fully proven (sem 82 Qed, pipe, stack,
  mutex, msgq, condvar, event, mem_slab, timer), 3 are 100%-`Admitted` stubs
  (poll 23, sched 24, thread_lifecycle 30 — counting `Admitted` tokens), and
  `heap_proofs.v` is an EMPTY file (0 lines) that still passes its test — so the
  CI job name "Rocq Proofs (13 files)" over-counted real content. **The executor
  had NO Rocq artifact at all — that column was empty.**

## STEP 2/3 — Model + theorem (new file: `proofs/executor_proofs.v`, 15 Qed)

Model (structurally parallel to `src/executor.rs`, correspondence documented
field-by-field in the file's comments; this is a model-level proof, stated so
in the file header):

- `ready : N` ↔ Rust `ready: u32` (bitmask; exact agreement argued — all handles
  < 8 < 32, so no u32-truncation divergence; `x & !(1<<h)` ↔ `N.ldiff x (mask h)`).
- `tstate : nat -> task_state` ↔ Rust `state: [TaskState; 8]`, with
  `task_state := Free | Pending | Done`.
- `wake s h` ↔ `Tasks::wake` (same guard: `h < MAX_TASKS && state[h] == Pending`,
  same bit-OR); `consume s h` ↔ `Tasks::consume` (same AND-NOT);
  `PickNext` ↔ `Tasks::pick_next(&self)` — non-mutating (shared borrow), modeled
  as the identity transition.
- `prio`/`deadline` omitted (no modeled operation writes them; documented).

Theorem statement (main, trace-quantified over arbitrary interleavings):

```coq
Theorem no_lost_wakeups : forall (t : list op) (s : exec_state) (i : nat),
  ready_bit s i = true ->
  ~ In (Consume i) t ->
  ready_bit (run s t) i = true.

Corollary delivered_wake_survives : forall (t : list op) (s : exec_state) (i : nat),
  (i < MAX_TASKS)%nat ->
  tstate s i = Pending ->
  ~ In (Consume i) t ->
  ready_bit (run (wake s i) t) i = true.
```

Supporting results: per-op twins of the Verus `ensures` clauses
(`wake_delivers`, `wake_preserves`, `consume_clears`, `consume_preserves_other`,
`step_tstate`), the bitmask lemmas twinning the Verus `by (bit_vector)` lemmas
(`mask_spec`, `lor_mask_self/other`, `ldiff_mask_self/other`), and the Rocq twin
of `Tasks::inv()` with trace closure (`step_preserves_inv`, `run_preserves_inv`).

## Compile output (verbatim)

Direct, with the repo toolchain's compiler:

```
$ coqc -Q <stdlib> Stdlib executor_proofs.v   # Rocq 9.0.1 (repo toolchain via rules_rocq_rust/Nix)
Closed under the global context
Closed under the global context
Closed under the global context
exit=0
```

(The three "Closed under the global context" lines are the file's closing
`Print Assumptions` on `no_lost_wakeups`, `delivered_wake_survives`,
`run_preserves_inv` — i.e. axiom-free, no holes.)

Through the repo's build gate (new target wired into `proofs/BUILD.bazel` and
added to the CI job list, job renamed to "Rocq Proofs (14 files)"):

```
$ bazel test //proofs:executor_proofs_test --test_output=all
INFO: From Compiling Coq proof proofs/executor_proofs.v:
Closed under the global context
Closed under the global context
Closed under the global context
==================== Test output for //proofs:executor_proofs_test:
Checking Rocq proof files...
  ✓ proofs/executor_proofs.v
Checking compiled .vo files...
  ✓ proofs/proofs/executor_proofs.vo
All 1 proof files verified.
//proofs:executor_proofs_test                                            PASSED in 0.4s
Executed 1 out of 1 test: 1 test passes.
```

## Admit count

```
$ grep -c Admitted proofs/executor_proofs.v
0
$ grep -c '^Qed\.' proofs/executor_proofs.v
15
$ grep -c Axiom proofs/executor_proofs.v
0
```

(Two lowercase `admit` substring hits exist — both are references to the Rust
method `Tasks::admit` in the honest-scope comment, not tactics.)

## STEP 4 — Honest-scope notes (also in README section "Executor no-lost-wakeups:
one property, three independent tracks" and in the file header)

- PROVEN: the model theorem — on the hand-written Rocq state machine mirroring
  `src/executor.rs`, a set ready bit survives every interleaving of
  wake/consume/pick_next that does not consume that task itself; plus the
  invariant (`ready` bits only on Pending slots, no bits ≥ MAX_TASKS) is
  trace-closed. Axiom-free, kernel-checked.
- NOT proven: any connection to the compiled artifact. No extraction, no
  refinement, no translation validation; model-to-Rust correspondence is a
  documented structural argument, human-checked.
- Out of the trace alphabet: `admit` (clears only a `Free` slot's bit — under the
  invariant already clear; cannot select a Pending slot) and `expire` (only ORs
  bits in — cannot lose a wakeup); `dispatch_one` carries the Verus frame
  `ready == old(ready)`. Rationale documented in the file header.
- Complement to Verus: same property already proven by Verus on the Rust source
  (`lemma_no_lost_wakeup` + per-mutator `ensures`) and bounded-checked by Kani on
  the executable path. Value of this file = diverse redundancy: two independent
  formalizations, two disjoint trusted bases (Z3 SMT vs the Rocq kernel).
- Docs updated for claim consistency: README (9→10 Rocq modules, new tri-track
  section, architecture diagram line), `docs/safety/verification-honesty.md`
  (10 fully proven, heap_proofs.v's emptiness now recorded), CI job name 13→14.
