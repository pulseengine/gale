# gust value-domain Health Monitor — diagnostic-coverage figure (VER-OS-HM-001)

**Status:** DELIVERED 2026-07-17; EXTENDED 2026-07-21 with the cross-sensor voting
detector (the last detector-class gap), alongside the fault-injection demonstrator
(`benches/gust/src/bin/gust_hm_probe.rs`) and the FSM proofs
(#189, Verus H1–H5 + Kani `h1_escalation_terminates` … `h7_replenish_requires_quiet`,
plus `h8_vote_2of3_characterized` for the TMR 2-of-3 vote).
This closes the second evidence part `REQ-OS-HM-001` asks for: *"The design SHALL
state a diagnostic-coverage figure against a named fault model, not merely assert
coverage qualitatively."*

## What this figure IS and IS NOT

**IS:** a coverage-of-the-named-model figure — every one of the 6 mission-loss causes
named in `REQ-OS-HM-001` (the original 5 external loss modes plus sensor-disagreement,
the cross-sensor TMR case) is mapped to a value-domain detector (`Fault` variant + gate)
in `gale::health_monitor`, and that mapping is exercised end to end (real `Hm` FSM,
real gates) by `gust_hm_probe`, reaching its correct terminal failsafe state within a
proven step bound. The mapping-to-terminal-state path is backed by the Verus/Kani
proofs in `src/health_monitor.rs` (#189): the escalation is total and provably
terminating (H1), never silently clears on a still-faulty observation (H2), and the
two terminal states are absorbing (H3) — no software path leaves `PartitionFailsafe`
except a liveness trip, and none leaves `CrossCoreTrip` at all.

**IS NOT:** a hardware/silicon-level FMEA diagnostic-coverage percentage (IEC 61508 /
ISO 26262 DC%, λ-based). This figure says nothing about sensor failure rates, silicon
fault models (stuck-at, bit-flip), or the probability a physical fault is actually
detected — that is a per-deployment FMEDA input, out of scope for a qemu logic
demonstrator. It is model coverage: does every named cause have a mapped detector
with a proven path to a terminal safe state? Yes — see below.

## The named fault model

Two axes, per `REQ-OS-HM-001` and `src/health_monitor.rs`:

- **6 named mission-loss causes** (`REQ-OS-HM-001`): RC-loss, datalink-loss,
  GPS/estimator-loss, geofence breach, low-battery/power-budget exhaustion, and
  sensor-disagreement (three redundant replicas failing the TMR 2-of-3 majority).
- **7 value-domain detector classes** (`Fault` in `src/health_monitor.rs`): `Stale`
  (freshness), `Implausible` (range/plausibility), `Diverged` (estimator innovation),
  `BudgetOverrun` (execution/resource budget), `DeadlineMiss` (output timeliness),
  `HeartbeatLoss` (liveness), `VoteMismatch` (cross-sensor TMR 2-of-3 voting).

## Coverage table

| named cause | mapped detector(s) | covered? | terminal state (proven, demonstrated) |
|---|---|---|---|
| RC-loss | `HeartbeatLoss` (`missed_beats` over limit on the RC link) | yes | `CrossCoreTrip` — liveness fault, cross-core trip (H3 `trips_cross_core`) |
| datalink-loss | `Stale` (telemetry `age_ms > limit_ms`) | yes | `PartitionFailsafe` — value fault, absorbed (H3) |
| GPS/estimator-loss | `Diverged` (`innov_abs > k_sigma`) | yes | `PartitionFailsafe` — value fault, absorbed (H3) |
| geofence breach | `Implausible` (position `value` outside `[lo, hi]`) | yes | `PartitionFailsafe` — value fault, absorbed (H3) |
| low-battery / power-budget exhaustion | `BudgetOverrun` (`used_us > budget_us`, read here as the power-budget window) | yes | `CrossCoreTrip` — liveness-class fault, cross-core trip (H3) |
| sensor-disagreement | `VoteMismatch` (three replicas `s0`/`s1`/`s2` fail the 2-of-3 majority within `vote_tol`) | yes | `PartitionFailsafe` — value fault, absorbed (H3) |

**6/6 named mission-loss causes covered** (target met).

Detector-class coverage: **6 of the 7** `Fault` classes are directly exercised by a
named cause above (`Stale`, `Implausible`, `Diverged`, `BudgetOverrun`, `HeartbeatLoss`,
and now `VoteMismatch` — cross-sensor voting, which had been the one detector class the
demonstrator did not exercise). `DeadlineMiss` (output-timeliness liveness) is the
remaining class: proven and gated identically (`gate_eval`/`step_fault` treat it exactly
like the other liveness faults, and it is exercised generically in the Kani harnesses
`h1_escalation_terminates` / `h6_long_run_restart_bound` / `h7_replenish_requires_quiet`
over `kani::any::<Fault>()`), but it is not the primary detector for any of the 6 *named*
mission-loss causes — it is an OS/partition-scheduler timing detector (a task meeting its
deadline late), not one of the external loss modes `REQ-OS-HM-001` names. It remains
available for other partitions/detectors the release line adds later.

## Two terminal states, by design (not a gap)

The escalation ladder is `Normal -> Degraded -> PartitionFailsafe -> CrossCoreTrip`,
but only the two *liveness* detectors (`HeartbeatLoss`, `BudgetOverrun`) can drive the
final step to `CrossCoreTrip` (`trips_cross_core` in `src/health_monitor.rs`). The
four *value* detectors (`Stale`, `Implausible`, `Diverged`, `VoteMismatch`) are absorbed
at `PartitionFailsafe` — proven in `lemma_failsafe_absorbing` (H3): a value-domain fault
cannot escalate further because the partition's outputs are already held safe; only a
liveness fault (the partition itself going unresponsive, or overrunning its window) is
grounds for the physically independent core to assert the hardware trip. This is
intentional containment design, confirmed both by proof and by the probe's absorbing
checks (each `PartitionFailsafe`-terminal cause is shown NOT to escalate further under
a continued same-kind fault, and NOT to accept a restart even with a healthy
observation).

## The proven latency bound

From `Hm::init()` (`Normal`, full `MAX_RESTARTS = 3` budget): `Normal`/`Degraded`
transitions in `step_fault`/`on_fault` are fault-KIND-agnostic (only the
`PartitionFailsafe` branch inspects `trips_cross_core`), so **any** fault kind reaches
`PartitionFailsafe` in exactly `MAX_RESTARTS + 1 = 4` `on_fault` calls (1 to enter
`Degraded` + `MAX_RESTARTS - 1 = 2` credit-burning faults + 1 exhausting fault). A
liveness fault reaches `CrossCoreTrip` in exactly one more call:
`MAX_RESTARTS + 2 = 5` total — this is `lemma_escalation_bound`'s proven bound
(`src/health_monitor.rs`), independently confirmed by `gust_hm_probe` reading the real
`Hm::state` at each step.

## Executable evidence

- `benches/gust/src/bin/gust_hm_probe.rs` — qemu (lm3s6965evb, cortex-m3) demonstrator.
  For each of the 6 named causes: builds the tripping `Obs`, confirms the mapped gate
  and `all_clear` both correctly fail, drives the REAL `Hm` FSM via `on_fault` (with a
  no-silent-clear `try_restart` check on the still-faulty observation), and confirms
  the correct terminal state is reached at the proven step bound, then confirms that
  terminal state is absorbing. A global non-vacuity check confirms a healthy `Obs`
  clears every gate (including every named cause's own gate), a `Normal` `Hm` stays
  `Normal` under a healthy quiet tick, and a `Degraded` `Hm` genuinely restarts to
  `Normal` on a healthy observation — a good frame never trips failsafe. A final
  standalone check re-drives `CrossCoreTrip` and applies all 7 `Fault` variants plus a
  healthy restart/quiet attempt, confirming none of them mutate the latched state.
- `src/health_monitor.rs` (#189) — the Verus H1–H5 proofs (total + terminating
  escalation, no-silent-clear, absorbing failsafe states, cause preservation, and the
  long-run cooldown-bounded-restart rate bound) plus the Kani harnesses that
  model-check the SAME shipped exec code (`plain/src/health_monitor.rs`) the probe
  calls into.

## The figure, stated plainly

**6/6 named mission-loss causes (RC-loss, datalink-loss, GPS/estimator-loss, geofence
breach, low-battery, sensor-disagreement) have a mapped value-domain detector and a
proven, demonstrated path to a terminal failsafe state; the 7 detector classes cover the
{freshness, range/plausibility, estimator-divergence, timing/resource-budget, deadline,
liveness, cross-sensor-voting} fault dimensions of the model, with 6 of those 7 classes
directly exercised by a named cause and the 7th (`DeadlineMiss`) proven identically but
not primary for any of the 6.**
This is a coverage-of-the-named-model figure computed from the cause->detector mapping
plus the FSM totality/termination/absorption proofs — it is not a hardware-level DC%
and makes no silicon-fault-rate claim; a per-deployment FMEDA remains a separate,
future input for any ASIL/DAL claim that requires one.
