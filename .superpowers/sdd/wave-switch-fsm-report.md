# Wave report — v0.6.0 outer partition-switch FSM core

Date: 2026-07-16
Branch: feat/gust-partition-switch-fsm (off main @ bc099e9)
Requirement: REQ-OS-SWITCH-001

## What landed

`src/partition_switch.rs` (Verus, no_std, scalar-only) — the OUTER fixed-priority
time-partitioned switch policy core:

- **MajorFrame**: static 4-window table (partition_id / offset / budget arrays +
  frame_len). `spec fn frame_inv` = contiguity + exact coverage of [0, frame_len)
  + all budgets > 0 (hardcoded-4 conjuncts, matching executor.rs's hardcoded-8
  style). Exec validator `check()` ensures `b == frame_inv()` (u64 sums, cannot
  overflow on hostile tables).
- **`current_window(t)`**: ensures containment AND uniqueness (forall over the
  other 3 windows, discharged via `lemma_window_disjoint`).
- **Switcher FSM**: `SwPhase { Running, SaveCtx, ProgramRegions, Resume }`,
  fields frame/cur/phase/swapped. All transitions are TOTAL (phase guard in the
  body, wdg-thin style — nothing load-bearing lives in a strippable requires):
  - `tick(t)`: Running -> SaveCtx IFF t is the last tick of the current window;
    **S1 non-maskable** — the ensures is an unconditional implication over every
    state/input; no disable path exists in the code (cannot-un-start mirrored).
    Boundary test uses `t == end - 1` (no u32 overflow; end >= 1 from budget > 0).
  - `mark_saved` / `mark_swapped` / `mark_resumed`: one-way pipeline steps.
  - **S2 ordering** as a state invariant: `swapped` ledger bit — cleared on the
    preemption edge, set ONLY by mark_swapped (ProgramRegions -> Resume); `inv()`
    carries (Resume ==> swapped) && (SaveCtx ==> !swapped) && (ProgramRegions ==>
    !swapped), plus `lemma_resume_implies_region_swap`. Region swap therefore
    strictly precedes resume; a stale swapped from the previous switch can never
    satisfy the Resume conjunct.
  - **S3 no-skip**: `mark_resumed` ensures cur advances by exactly one (mod 4);
    `lemma_no_skip` proves the successor tick of a boundary lands in exactly
    window (cur+1) % 4 — the frame is followed, no window skipped or repeated.
- **Trusted seams**: `unsafe extern "C" { ctx_save, region_swap, ctx_resume }`
  outside the verus! block; three `#[verifier::external_body]` thin wrappers with
  NO ensures (no proof leans on hardware behavior); single narrowly-scoped
  `#[allow(unsafe_code)]` per seam against the crate-wide deny. `run_switch()`
  (verified) interleaves the seam calls with the verified mark_* steps, so the
  swapped invariant machine-checks region-programming-before-resume in the one
  place the seams are crossed. `region_swap` is built against the contract only
  (I-ISO integration wires it to program_partition later).

## Wiring

- `src/lib.rs`: `pub mod partition_switch;`
- BUILD.bazel `VERUS_SRCS` += src/partition_switch.rs
- plain mirror regenerated via tools/verus-strip (never hand-edited):
  plain/src/partition_switch.rs + plain/src/lib.rs
- Both convergence lists: plain/BUILD.bazel `verus_strip_gate` (verus_srcs +
  plain_srcs) and tools/verus-strip/tests/gate.rs FILES.

## Gates (real output, exit codes checked)

1. `bazel test //:verus_test --test_output=all` -> PASSED,
   "verification results:: 1095 verified, 0 errors" (baseline main: 1081 — +14,
   no regressions).
2. Strip gate: `cargo test --manifest-path tools/verus-strip/Cargo.toml --test
   gate` -> ok, 2 passed; 0 failed (both convergence tests).
3. Kani (plain crate, cargo kani --harness ...):
   - k1_nonmaskable_boundary        -> VERIFICATION:- SUCCESSFUL (0 of 87 failed)
   - k2_resume_only_via_program_regions -> VERIFICATION:- SUCCESSFUL (0 of 145 failed)
   - k3_frame_covers_exactly_one    -> VERIFICATION:- SUCCESSFUL (0 of 209 failed)
   - k4_no_skip_advances_by_one     -> VERIFICATION:- SUCCESSFUL (0 of 178 failed)
4. No assume, no weakened/removed ensures, no vacuous postconditions.
   `bazel test //:cargo_test` -> PASSED.

## Pre-existing local failures (NOT regressions — verified on clean main via stash)

- `//:fmt_test` and `//:clippy_test` fail identically on clean main (local
  rustfmt/clippy 1.97 vs the prettyplease-generated plain/ mirrors — e.g.
  plain/src/executor.rs itself diffs; clippy's new byte_char_slices lint fires
  in tests/differential_cbprintf.rs). Untouched by this wave; the strip gate is
  the authoritative mirror-convergence check and hand-formatting plain/ would
  break it.

## Notes for integration (v0.6.x follow-ons)

- `region_swap(part)` is called with the INCOMING partition id, after ctx_save
  of the outgoing and before ctx_resume of the incoming — wire to the I-ISO
  core's program_partition behind exactly this contract.
- `MajorFrame::check()` is the seam for validating the static frame table once
  at init; `Switcher::new` requires the proven invariant thereafter.
