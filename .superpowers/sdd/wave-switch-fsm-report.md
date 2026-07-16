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
  (verified) interleaves the seam calls with the verified mark_* steps; the
  swapped invariant machine-checks the FSM ordering (mark_swapped strictly
  before mark_resumed) — the binding of `seam_region_swap` to `mark_swapped`
  (that the seam call is actually issued on that edge) is trusted code order in
  run_switch's body, not machine-checked. `region_swap` is built against the
  contract only (I-ISO integration wires it to program_partition later).

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

- `//:fmt_test` and `//:clippy_test` fail on clean main too (local
  rustfmt/clippy 1.97 vs the prettyplease-generated plain/ mirrors; clippy's
  first-failing unit differs by compile-order nondeterminism, and all clippy
  errors are in files untouched by this branch). Not exactly identical: the
  branch ADDS plain/src/partition_switch.rs to the rustfmt-diff list — one more
  file of the same pre-existing generated-mirror class (gate was already red;
  no regression). The strip gate is the authoritative mirror-convergence check
  and hand-formatting plain/ would break it. Note the CI lint path is
  `cargo clippy --all-targets` (bazel-tests.yml:51), not these bazel targets.

## Notes for integration (v0.6.x follow-ons)

- `region_swap(part)` is called with the INCOMING partition id, after ctx_save
  of the outgoing and before ctx_resume of the incoming — wire to the I-ISO
  core's program_partition behind exactly this contract.
- `MajorFrame::check()` is the seam for validating the static frame table once
  at init; `Switcher::new` requires the proven invariant thereafter.
- Model scoping: non-maskability is proven at the FSM-POLICY level.
  System-level non-maskability additionally depends on (a) the trusted tick
  source delivering the boundary tick and (b) the switch pipeline completing
  before the next boundary — a boundary tick arriving while the FSM is
  mid-pipeline (non-Running) is a no-op by design, and switch latency vs.
  window length is unmodeled.
- FV-track scoping: this module's formal verification is Verus + Kani; there
  is no Rocq track for partition_switch (consistent with the executor.rs
  precedent — the Rocq track covers the older kernel primitives). Release or
  claim text must not describe this module as verified on all three tracks.

## Fix wave

Adversarial review (.superpowers/sdd/wave-switch-fsm-review.md, verdict
FIX_REQUIRED: 2 important, 5 minor) — all findings addressed or adjudicated:

- **I1 (honest scoping)**: the run_switch doc (src/partition_switch.rs and the
  plain mirror) and the trusted-seams paragraph of this report now claim only
  what is machine-checked — the FSM ordering, `mark_swapped` strictly before
  `mark_resumed` — and state explicitly that the binding of `seam_region_swap`
  to `mark_swapped` (that the seam call is actually issued on that edge) is
  trusted code order in run_switch's body, not machine-checked. This matches
  the file-top trusted-seam note, which was already correct. Plain mirror
  regenerated via tools/verus-strip (doc-comment-only delta).
- **I2 (rivet artifact)**: artifacts/gust_partition_switch.yaml added —
  REQ-OS-SWITCH-001 (sw-req, implemented, release v0.6.0; derives-from
  SYSREQ-BYOOS-001, related-to REQ-OS-EXEC-001 / REQ-OS-MPU-001) plus
  VER-OS-SWITCH-001 (sw-verification, formal-verification, verifies
  REQ-OS-SWITCH-001; Verus+Kani scope and the seam-binding trust boundary
  stated per I1). `rivet validate` → PASS; zero warnings attach to the new
  artifacts (the 331 baseline warnings are the pre-existing class).
- **M1**: the pre-existing-failures paragraph above reworded — not "identical":
  the branch ADDS plain/src/partition_switch.rs to the rustfmt-diff list (same
  generated-mirror class; gate already red on main, no regression); clippy
  first-fail differs by compile-order nondeterminism; CI lint path noted.
- **M2**: NOT fixed here — the referenced plan doc
  (docs/superpowers/plans/2026-07-15-gust-safety-release-line.md) has no
  reference anywhere on this branch to correct; landing the plan doc is an
  orchestrator action (review agrees: "Orchestrator: land the plan").
- **M3**: FV-track scoping bullet added above (Verus + Kani, no Rocq).
- **M4**: mid-pipeline-tick / switch-latency model-scoping bullet added above.
- **M5**: note-only per review (pub fields match the executor.rs convention,
  and the Kani harnesses need literal construction); no change.

Gates re-run after the fixes (fresh, exit codes checked):

1. `bazel test //:verus_test --nocache_test_results` → PASSED,
   "verification results:: 1095 verified, 0 errors".
2. Strip gate → ok, 2 passed; 0 failed (mirror regenerated, convergent).
3. Kani: k1 0/87, k2 0/145, k3 0/209, k4 0/178 failed — all four
   VERIFICATION:- SUCCESSFUL.
4. `rivet validate` → PASS (no new warnings vs baseline).

No assume added, no ensures weakened; the fix wave touches doc comments, the
report, and rivet artifacts only — zero executable or spec changes.
