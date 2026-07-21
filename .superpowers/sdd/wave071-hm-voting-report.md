# wave071 — HM cross-sensor voting detector (VER-OS-HM-001 → verified)

Finished the cross-sensor voting detector for gale's verified Health Monitor,
closing v0.7.0. Built on the SOUND partial at `7b6d1e7` (which added
`Fault::VoteMismatch`, the `Obs` sensor fields, the `agree`/`vote_clears` specs,
`vote_ok` exec, and the `gate_clears`/`all_gates_clear` ghost arms) — that partial
did not compile end-to-end because the exec twins and every Fault-exhaustive site
still lacked the vote arm.

## What remained and how it was closed

1. **`gate_eval` exec** — added `Fault::VoteMismatch => vote_ok(obs.s0, obs.s1,
   obs.s2, obs.vote_tol)` (the match was non-exhaustive without it). Verus proves
   it equals `gate_clears`.
2. **`all_clear` exec** — conjoined `&& vote_ok(...)` so it mirrors
   `all_gates_clear`.
3. **Fault-exhaustive ripple** — the only exhaustive `match cause` sites are
   `gate_clears` (spec, already had the arm) and `gate_eval` (exec, fixed here).
   All other Fault uses are non-exhaustive guards (`trips_cross_core` via `||`,
   `on_fault`'s `matches!(f, HeartbeatLoss | BudgetOverrun)`) — VoteMismatch
   correctly falls through as a NON-liveness value fault (absorbed at
   PartitionFailsafe, never cross-trips), which is the intended TMR design.
   - **Which lemmas needed the vote arm:** none needed a *hand-written* new case.
     `lemma_all_clear_implies_cause_gate` re-establishes the vote case
     automatically because `all_gates_clear` now includes `vote_clears` and
     `gate_clears(VoteMismatch,·) == vote_clears(·)` — exactly the "follows like
     the others" path. The H1/H3/H4/H5 lemmas quantify over `step_fault`, which is
     cause-agnostic, so they re-verify unchanged. The Kani `any_fault()` was
     widened `%6 → %7` to include VoteMismatch, and `any_obs()` extended with the
     four new sensor fields, so h1/h2/h3/h4/h6/h7 now exercise the vote path too.
4. **Kani harness** — added `h8_vote_2of3_characterized`: over `kani::any()` i32
   s0/s1/s2/tol it asserts `vote_ok == ` a hand-computed 2-of-3 oracle (pairwise
   `|diff| <= tol` in exact i64), plus explicit all-agree ⇒ true (for tol ≥ 0) and
   all-disagree ⇒ false discrimination. Also proves the i64-widening is
   overflow-safe for every i32 input.
5. **plain mirror** — regenerated `plain/src/health_monitor.rs` via
   `tools/verus-strip` (not hand-edited); health_monitor already in the strip gate
   FILES + plain lib.
6. **Obs construction sites** — the only literals are `any_obs()` (src, fixed) and
   the probe's `healthy_obs()` (fixed: three AGREEING replicas s0=s1=s2=50,
   vote_tol=2). No test literals elsewhere.
7. **Probe VoteMismatch cause** — added `sensor-disagreement` (s0=0/s1=1000/s2=2000,
   every pairwise diff > tol) driving VoteMismatch → PartitionFailsafe within the
   proven bound; added VoteMismatch to `all_faults()` (now `[Fault; 7]`) and the
   non-vacuity mapping table. OK line notes the voting detector is now exercised.
8. **DC doc** — 7 detector classes (voting was the gap), 6 named causes each with a
   mapped detector + proven terminal path; 6/7 classes named-exercised (DeadlineMiss
   the remaining proven-generically one). Kept the "model-coverage, not FMEDA-λ"
   framing.
9. **rivet** — flipped REQ-OS-HM-001 and VER-OS-HM-001 to `verified`; removed the
   "STATUS implemented, NOT yet verified … cross-sensor voting" caveat block;
   appended the one-line VERIFIED (2026-07-21) resolution.

## Gate results (verbatim, all exit 0)

- **Verus** `bazel test //:verus_test --test_output=all --cache_test_results=no`:
  `verification results:: 1160 verified, 0 errors` → **PASSED in 5.9s**.
  Obligation delta: baseline (pre-voting `a397fdb` src) = **1159 verified**;
  now **1160 verified** → **+1 verified unit** (the new `vote_ok` exec fn, proven
  against `ensures ok == vote_clears(...)`). The exhaustive-match arms
  (`gate_eval`/`all_clear`/`gate_clears`/`all_gates_clear`) and
  `lemma_all_clear_implies_cause_gate` absorbed the vote arm within their existing
  units — a real new proof, not a no-op.
- **verus-strip gate** `cargo test --manifest-path tools/verus-strip/Cargo.toml
  --test gate`: `test result: ok. 2 passed; 0 failed` → **2/2**.
- **Kani** (each SUCCESSFUL):
  - h8_vote_2of3_characterized — 1.05s (0 of 13 checks failed)
  - h1_escalation_terminates — 0.076s
  - h1_trip_bound — 0.032s
  - h2_no_silent_clear — 2.27s
  - h3_absorbing_failsafe — 0.128s
  - h4_cause_preserved — 0.486s
  - h5_gates_characterized — 0.544s
  - h6_long_run_restart_bound — 8.41s
  - h7_replenish_requires_quiet — 1.36s
- **cargo build** — clean, exit 0.
- **cargo clippy --lib** — clean after adding `nonminimal_bool = "allow"` to the
  sanctioned verus-strip lint block in Cargo.toml (the canonical TMR 2-of-3 form
  `(a01&&a02)||(a01&&a12)||(a02&&a12)` is kept explicit; clippy's "simplification"
  is uglier and generated code cannot be hand-edited).
- **rivet validate** — `Result: PASS (332 warnings)`, exit 0 (warnings are
  pre-existing SYSREQ sys-integration-verification coverage gaps, unrelated).

## Probe OK line (qemu lm3s6965evb, cortex-m3)

```
gust-hm-probe OK: 6/6 named mission-loss causes (RC-loss, datalink-loss,
GPS/estimator-loss, geofence-breach, low-battery, sensor-disagreement) each reached
their mapped terminal state (2 CrossCoreTrip via HeartbeatLoss/BudgetOverrun,
4 PartitionFailsafe via Stale/Diverged/Implausible/VoteMismatch — the cross-sensor
TMR 2-of-3 voting detector is now exercised end to end) within the proven bound
(MAX_RESTARTS+1=4 / MAX_RESTARTS+2=5 on_fault steps), no silent clear on any
still-faulty restart attempt, non-vacuous healthy-frame check passed, CrossCoreTrip
confirmed absorbing over all 7 fault kinds
```
The `sensor-disagreement` line: `-> value-domain fault -> PartitionFailsafe in 4
on_fault steps (bound MAX_RESTARTS+1=4)`.

## Honest gaps

- **Verus delta is +1 verified UNIT**, not a large jump — Verus counts one unit per
  function, so the vote arm folded into existing units (`gate_eval`, `all_clear`,
  the lemmas) rather than each producing a separate counter. The genuine new proof
  content is the overflow-safe `vote_ok` ↔ `vote_clears` equivalence and the
  re-discharge of the exhaustive matches; it is real, just not many counters.
- **Task-summary number drift:** the task headline said "5/5 named causes" / "6/6
  detector classes"; the actual delivered state is **6 named causes** (a 6th,
  sensor-disagreement, was added per task item 7) and **7 detector classes** with
  **6/7 named-exercised**. VoteMismatch is the newly-closed gap; DeadlineMiss
  remains the one class proven generically (Kani `any_fault`) but not tied to a
  named external loss mode. The docs/rivet/probe are all internally consistent on
  6 causes / 7 classes.
- **Scope unchanged from #189/#199:** the DC figure is model-coverage against the
  named fault list, NOT a hardware FMEDA DC%/λ claim; qemu logic demonstrator, not
  silicon. Rocq track still does not cover this module (Verus + Kani only, as
  recorded).
- **No shortcuts:** no `assume`/`admit`/`external_body`/broadened-requires were
  used to pass any proof.
