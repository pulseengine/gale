# wave070 — gust v0.7.0-CLOSING: per-cause fault-injection demonstrator + DC figure (VER-OS-HM-001)

Branch: `feat/gust-hm-failsafe-demo`
Deliverable: `benches/gust/src/bin/gust_hm_probe.rs` + `docs/gust/hm-diagnostic-coverage.md`
+ the `VER-OS-HM-001` rivet flip. **No verified source was touched**
(`src/health_monitor.rs` and `plain/src/health_monitor.rs` are exactly as merged
in #189), so no Verus/Kani/strip rerun was required — confirmed by `git status`
before commit: only the new probe bin, the new doc, and the artifact YAML are
in the diff.

## Cause -> detector mapping used

A demonstrator concern, kept in the probe only (not added to the verified src,
which stays cause-agnostic over `Fault`):

| named cause (REQ-OS-HM-001) | mapped `Fault` | class | terminal state |
|---|---|---|---|
| RC-loss | `HeartbeatLoss` (`missed_beats` over limit) | liveness | `CrossCoreTrip` |
| datalink-loss | `Stale` (`age_ms > limit_ms`) | value | `PartitionFailsafe` |
| GPS/estimator-loss | `Diverged` (`innov_abs > k_sigma`) | value | `PartitionFailsafe` |
| geofence breach | `Implausible` (`value` outside `[lo, hi]`) | value | `PartitionFailsafe` |
| low-battery | `BudgetOverrun` (`used_us > budget_us`, read as the power budget per REQ's own wording "low-battery/power-budget exhaustion") | liveness | `CrossCoreTrip` |

Two different terminal states is the core's PROVEN, INTENDED behaviour, not a
probe shortcoming: `src/health_monitor.rs`'s `trips_cross_core` only fires for
`HeartbeatLoss`/`BudgetOverrun`; a value-domain fault (`Stale`/`Implausible`/
`Diverged`) is absorbed at `PartitionFailsafe` forever under the same-kind
fault (`lemma_failsafe_absorbing`, H3) — the partition's outputs are already
held safe, so only a liveness fault (the partition itself becoming
unresponsive or overrunning) is grounds for the physically independent core to
assert the hardware trip. `DeadlineMiss` is the one detector class not
primary for any of the 5 named causes (it is an OS/scheduler timeliness
detector, proven identically, exercised generically in the Kani harnesses over
`kani::any::<Fault>()`) — stated as an honest note in the DC doc, not hidden.

## Latency bound + where it comes from

Derived directly from the core's own structure/proofs, not re-derived: from
`Hm::init()` (`Normal`, full `MAX_RESTARTS = 3` budget), the `Normal`/`Degraded`
transitions in `on_fault` are fault-KIND-agnostic (only the `PartitionFailsafe`
branch inspects `trips_cross_core`), so ANY fault kind reaches
`PartitionFailsafe` in exactly `MAX_RESTARTS + 1 = 4` `on_fault` calls, and a
liveness fault reaches `CrossCoreTrip` in exactly one more:
`MAX_RESTARTS + 2 = 5` — this is `lemma_escalation_bound`'s proven bound in
`src/health_monitor.rs`. The probe confirms this bound by reading the REAL
`Hm::state` at each step (no shadow FSM).

## Verbatim probe OK output

```
$ cargo run --bin gust_hm_probe --release
    Finished `release` profile [optimized] target(s) in 0.03s
     Running `qemu-system-arm -cpu cortex-m3 -machine lm3s6965evb -nographic -icount shift=1 -semihosting-config enable=on,target=native -kernel target/thumbv7m-none-eabi/release/gust_hm_probe`
gust-hm-probe: driving the verified value-domain Health Monitor per named mission-loss cause
  RC-loss -> HeartbeatLoss/BudgetOverrun-class -> CrossCoreTrip in 5 on_fault steps (bound MAX_RESTARTS+2=5), absorbing confirmed
  datalink-loss -> value-domain fault -> PartitionFailsafe in 4 on_fault steps (bound MAX_RESTARTS+1=4), absorbing under continued fault + healthy restart attempt confirmed
  GPS/estimator-loss -> value-domain fault -> PartitionFailsafe in 4 on_fault steps (bound MAX_RESTARTS+1=4), absorbing under continued fault + healthy restart attempt confirmed
  geofence-breach -> value-domain fault -> PartitionFailsafe in 4 on_fault steps (bound MAX_RESTARTS+1=4), absorbing under continued fault + healthy restart attempt confirmed
  low-battery -> HeartbeatLoss/BudgetOverrun-class -> CrossCoreTrip in 5 on_fault steps (bound MAX_RESTARTS+2=5), absorbing confirmed
  non-vacuity: healthy Obs clears all 6 gates + every mapped cause's gate; Normal stays Normal; Degraded restarts to Normal — a good frame does not trip failsafe
  termination: CrossCoreTrip absorbed all 6 fault kinds + a healthy restart attempt + a healthy quiet tick — no software path out
gust-hm-probe OK: 5/5 named mission-loss causes (RC-loss, datalink-loss, GPS/estimator-loss, geofence-breach, low-battery) each reached their mapped terminal state (2 CrossCoreTrip via HeartbeatLoss/BudgetOverrun, 3 PartitionFailsafe via Stale/Diverged/Implausible) within the proven bound (MAX_RESTARTS+1=4 / MAX_RESTARTS+2=5 on_fault steps), no silent clear on any still-faulty restart attempt, non-vacuous healthy-frame check passed, CrossCoreTrip confirmed absorbing over all 6 fault kinds
```
Exit code: `0`.

## Gates + exit codes

| gate | result | exit |
|---|---|---|
| `cargo run --bin gust_hm_probe` (release, qemu runner) | OK line above | 0 |
| Regression: `cargo run --bin gust_switch_probe` (release) | `gust-switch-probe OK: 3 partitions across the major frame ... 5 expected cross-partition faults denied` | 0 |
| `cargo build --bins --release` | clean (1 pre-existing warning in unrelated `gust_breadth_probe.rs`, `unnecessary unsafe block` — not touched by this change) | 0 |
| `rivet validate` | `Result: PASS (332 warnings)` — same warning count as the pre-change baseline; no new warning on `VER-OS-HM-001`/`REQ-OS-HM-001` | 0 |
| `git status` diff scope | `benches/gust/src/bin/gust_hm_probe.rs` (new), `docs/gust/hm-diagnostic-coverage.md` (new), `artifacts/gust_safety_release_line.yaml` (modified) — **no** `src/health_monitor.rs` or `plain/` touched | n/a |

## The DC figure

**5/5 named mission-loss causes (RC-loss, datalink-loss, GPS/estimator-loss,
geofence breach, low-battery) have a mapped value-domain detector and a
proven, demonstrated path to a terminal failsafe state; the 6 detector classes
cover the {freshness, range/plausibility, estimator-divergence,
timing/resource-budget, deadline, liveness} fault dimensions of the model,
with 5 of those 6 classes directly exercised by a named cause and the 6th
(`DeadlineMiss`) proven identically but not primary for any of the 5.**

Stated honestly in `docs/gust/hm-diagnostic-coverage.md`: this is a
coverage-of-the-named-model figure (cause -> detector mapping + FSM
totality/termination/absorption proofs), NOT a hardware/silicon-level FMEA
DC%/λ figure — that remains a per-deployment FMEDA input, out of scope for a
qemu logic demonstrator.

## Rivet status flips + reasoning

- **`VER-OS-HM-001`: `proposed` -> `verified`.** Both parts of its description
  are now closed: (1) the Verus H1-H5 + Kani harnesses (#189, already merged,
  untouched) prove the escalation ladder total/terminating/no-silent-clear/
  absorbing/cause-preserving/rate-bounded; (2) the fault-injection demo now
  exists end-to-end and the DC figure is stated. Appended a DELIVERED
  (2026-07-17) paragraph to its description per the task; `rivet validate`
  still PASSes.
- **`REQ-OS-HM-001`: left at `implemented` (NOT flipped to `verified`).**
  Reasoning: the REQ's description asks for MORE than what `gust_hm_probe`
  and the core cover. Two specific gaps I found on re-reading it against
  `src/health_monitor.rs`:
  1. The REQ names "cross-sensor voting" as one of the required value-domain
     detectors alongside freshness/plausibility/innovation. The verified core
     has no cross-sensor-voting gate — only single-stream `fresh`/
     `plausible`/`innovation_ok`/budget/deadline/heartbeat gates. That
     detector class is simply not implemented, so the REQ is not fully
     closed by what exists today.
  2. The REQ says the failsafe ladder for all 5 named causes "each mapping
     onto the existing independent cross-core monitor tier." Taken literally,
     that reads as every named cause ending at the cross-core tier
     (`CrossCoreTrip`); what is actually proven and demonstrated is that only
     the 2 liveness-mapped causes reach `CrossCoreTrip` — the 3 value-mapped
     causes correctly terminate at `PartitionFailsafe` (by design, H3) and
     never reach the cross-core tier via software. This is very likely the
     *correct* systems-safety design (holding a partition's outputs safe
     locally is strictly less drastic than forcing a hardware cross-core
     trip, and is what the proofs make sound), but it is a stronger and more
     specific claim than the REQ's literal wording, so I did not want to
     silently declare the REQ's letter fully met.
  Given both, and per the task's explicit preference to lean conservative, I
  left `REQ-OS-HM-001` at `implemented` and flipped only the verification
  artifact. `VER-OS-HM-001` verifies exactly what it says it verifies (the
  FSM properties + the demonstrator + the DC figure against the named fault
  list) — it does not claim the REQ's cross-sensor-voting detector exists.

## Honest gaps

- **qemu, not silicon.** `gust_hm_probe` runs on lm3s6965evb (cortex-m3) under
  qemu-system-arm — logic/FSM correctness evidence, not a silicon timing or
  physical-fault-injection demonstration (same honesty posture as
  `gust_switch_probe` before it).
- **DC figure is model-coverage, not FMEDA-λ.** Explicitly labelled as such in
  the doc's opening section — it says nothing about sensor/silicon failure
  rates or physical fault models.
- **Cross-sensor voting is not implemented anywhere in the core** — REQ-OS-HM-001
  names it, `src/health_monitor.rs` does not have it. This is why REQ stays
  `implemented` rather than `verified` (see above).
- **`DeadlineMiss` has no named-cause mapping.** It is proven and Kani-checked
  identically to the other liveness faults, but none of the 5 REQ-named
  causes maps to it primarily; noted explicitly in the DC doc rather than
  silently omitted or force-fit onto one of the 5 causes.
- **Only 2/5 causes reach the literal cross-core tier.** Discussed above under
  the REQ-flip reasoning — an honest reading of "each mapping onto the
  existing independent cross-core monitor tier" vs. what the proven FSM
  actually does (3 of 5 correctly terminate one tier earlier, at
  `PartitionFailsafe`, by proven design).
