# gust safety release line — the load-bearing build (BUILD path)

**Status:** APPROVED (user review 2026-07-16) — the standing release-line reference.
**Date:** 2026-07-15 (approved + status-refreshed 2026-07-16). **Owner:** gale/gust.

> **Execution status at approval (2026-07-16):** v0.4.0 SHIPPED (#192 — both syscall-seam
> probes green on synth 0.45.1, the fused-path miscompile root-caused as synth#757 and fixed
> upstream). v0.5.0 SHIPPED (#193 — `VER-OS-ISO-001` **verified**; the I-ISO keystone
> delivered exactly as planned, incl. the synth#757 containment demo as the flagship
> acceptance test). v0.6.0 CORE LANDED (#195 — `VER-OS-SWITCH-001` **implemented**;
> single-core 3-partition demonstrator green, multi-core Renode placement is the remaining
> v0.6.x item). ordeal-BV pilots 1–4 landed on T1's certificate thesis. T3/T4 specs filed
> upstream (spar#331 / synth#778).
**Decision context:** the build-vs-host fork (gale#63 spec §12) is resolved to **BUILD** —
verified all the way down, no outside dependency, gale owns and controls the whole stack.
This plan sequences the *safety-load-bearing* build from what is shipped today to a
safety-capable v1.0. It is the release-planning half of the feature loop: every increment is
rivet-led, oracle-gated, and states what assurance it delivers **and** what it does not yet
claim.

## 0. Governing principles (from the merged spec, rev 6)

1. **BUILD — vertical, all from us.** The outer preemptive switch + MPU/MMU region-swap +
   Health-Monitor core are gale's own tiny verified sliver; we do not host on a COTS 653
   kernel. "We do it and we control it" is part of the argument — no unverifiable blob in
   the TCB.
2. **The thesis: checkable certificates at *every* layer** — codegen (synth BIN-VERIFY /
   RULE-VERIFY), isolation (I-ISO: hardware contains the compiler), timing (spar
   Lean-verified WCRT), proof-obligations (ordeal LRAT). Each layer is untrusted-fast-tool +
   small-trusted-checker. This symmetry, not any single piece, is the load-bearing argument.
3. **Silicon-independent by construction** (spec §1.1): author once in the Component Model,
   dissolve to any synth target (Cortex-M / AArch64 / RISC-V), isolate via that target's
   MPU/MMU/PMP. The cert *level* is a per-target output, not a gate on the line.
4. **Two-column evidence ledger** — track *functional-correctness* evidence and
   *isolation/safety* evidence separately; never let one stand in for the other.
5. **Maturing compiler, not a blocker** — synth is mid-bootstrap (transpiler → formal-proof
   compiler); uncovered paths (e.g. synth#757) are coverage-extension work on a curve, and
   I-ISO decouples compiler maturity from isolation assurance.

## 1. Where we stand (the shipped baseline)

| capability | status | evidence column |
|---|---|---|
| Inner cooperative async executor (tickless, fixed-priority) | **shipped** (PR #176) | functional: Verus 1081/0, Kani 2/2, qemu-probed |
| 10 verified thin-seam drivers (gpio/timer/spi/uart/i2c/adc/dac/wdg/pwm/can) | **shipped** | functional: Kani 7/7 each, 0-SRAM, 0 new TCB atoms |
| F100 cross-core Health-Monitor tier | **demonstrable** (Renode 3-core, gale#65 MU) | safety: HW-independent failsafe |
| Tri-track infra (Verus, Kani; Rocq stubbed), ordeal LRAT (gale#173) | **partial** | functional; isolation-safety = **0** |

**Honest baseline: isolation/safety evidence = 0.** Everything shipped is the *functional*
leg. This release line builds the *isolation/safety* leg.

## 2. The release line

Each release names the **load-bearing property** it delivers, the deliverables, the **oracle
gate** (mechanical, per oracle-gate-a-change), and the rivet requirement/verification IDs
that must close its V. Releases are sequential on the critical path; the cross-cutting
tracks (§3) thread across them.

### v0.4.0 — Buffer-carrying syscall seam (the OS-service boundary)
- **Load-bearing property:** a partition can invoke OS services (`gust:os {time, log, spawn}`)
  across a real capability boundary — the seam the whole two-level model calls through.
- **Deliverables:** the fused tl-node (time+log buffer) and ts-node (time+spawn) dissolve
  cleanly and run correctly; the `gust:os` WIT world callable from a dissolved app.
- **Oracle gate:** `gust_os_tl_probe` (log == "gust:os up\n") + `gust_os_ts_probe` (poll==1)
  green on qemu; Kani on the seam FSM; rivet `REQ-OS-SYSCALL-001` → `VER-OS-SYSCALL-001`.
- **Gated on:** synth#757 coverage completion (the fused-path miscompile; synth-side
  #760/THM_CALL-oracle work + our probe as the acceptance test). This is **track T2** landing
  its first coverage milestone.
- **Assurance delivered:** functional (the call path is correct). **Not yet:** isolation.

### v0.5.0 — I-ISO core: verified MPU/MMU region-swap (the isolation keystone)
- **Load-bearing property:** **the first freedom-from-interference** — a hardware
  memory-protection boundary, programmed by the verified core on entry to a partition,
  physically denies cross-partition access. This *discharges* invariant I-ISO.
- **Deliverables:** the region-table + `program_regions_on_switch` as an mmio/scalar-FSM
  sliver (thin-seam discipline); Verus/Kani proof that the programmed regions are disjoint,
  cover the partition, and deny everything else; the ARMv7-M MPU instantiation first, with
  the AArch64 MMU+EL2 path scoped.
- **Oracle gate:** Verus + Kani on the region FSM (disjoint / total / deny-by-default);
  Renode/qemu **fault-injection demo** — a cross-region write BusFaults; **the synth#757
  containment demo (flagship):** a scripted compiler-introduced cross-partition write is
  physically denied by the MPU — I-ISO from claim to filmed demonstration. rivet
  `REQ-OS-MPU-001` / `VER-OS-MPU-001`.
- **Assurance delivered:** freedom-from-interference for a **single** partition boundary —
  the first real isolation evidence in the project.

### v0.6.0 — Outer preemptive partition switch (the ARINC-653 backbone, built small)
- **Load-bearing property:** **temporal + spatial isolation across N partitions** —
  non-maskable window-end preemption (temporal) + per-partition region-swap (spatial, from
  v0.5.0), on an offline-planned major frame.
- **Deliverables:** the verified switch core; the fixed-priority time-partitioned major-frame
  scheduler; multi-partition-per-core (flight + mission on M7; estimator on M4); the 3-core
  mapping wired in the Renode model. Contribute the **proof**; **adopt** the mechanism
  (established static-partitioning hypervisor patterns + textbook MPU/MMU) — no novel scheduler.
- **Oracle gate:** Verus/Kani on the switch (window-end preemption un-maskable by inner code;
  region-swap ordered before inner resume); the two-level demo green on Renode 3-core;
  schedulability via **track T3** (spar). rivet `REQ-OS-SWITCH-001` / `VER-OS-SWITCH-001`.
- **Assurance delivered:** the full outer isolation layer — where the safety case lives.

### v0.7.0 — Value-domain Health Monitor + failsafe state machine
- **Load-bearing property:** the HM catches **erroneous-but-timely** output (the dominant
  real loss mode), not just liveness — the gap the expert panel flagged.
- **Deliverables:** value-domain detectors (data-freshness/staleness, innovation/plausibility
  gating, sensor voting, estimator-divergence); an explicit failsafe FSM (RC-loss,
  datalink-loss, GPS/estimator-loss, geofence, low-battery) mapping onto the F100 tier-2 trip;
  the tiered escalation proven terminating.
- **Oracle gate:** Verus/Kani on the HM/failsafe FSM; fault-injection demo per failsafe
  trigger; a stated **diagnostic-coverage** figure against a named fault model. rivet
  `REQ-OS-HM-001` / `VER-OS-HM-001`.
- **Assurance delivered:** the supervision leg of the safety case (the F100 backstop already
  demonstrable; this makes the value-domain argument real).

### v1.0 — Safety-capable release
- **Load-bearing property:** a **closed V-model safety case** for the two-level partition OS
  on the target silicon, at the per-target certification level (§1.1).
- **Deliverables:** full 3-core RT1176 mapping; all tracks (§3) complete; the two-column
  ledger fully populated (functional **and** isolation/safety both substantial); the rivet
  traceability audit closed (every approved requirement → architecture → implementation →
  verification, up the right side); the ASIL-D/DAL decomposition; per-target cert-level output
  and the RT1176 interference characterization (spec §11).
- **Oracle gate:** rivet `traceability-audit` / `release-execution` completeness gate green;
  the full Renode/silicon demonstrator; the safety-case document signed (sigil).

## 3. Cross-cutting tracks (thread across the releases)

These are not sequential releases — they mature in parallel and each **gates** the assurance
claim of the releases above.

- **T1 — Tri-track completion (make "Verus + Rocq + Kani" real).** Close ≥1 load-bearing
  theorem on the **Rocq** track (candidate: no-lost-wakeups, or the v0.5.0 region-disjointness
  theorem) so the tri-track claim is *demonstrated*, not aspirational (spec §7.1). The
  academic machine-checked-schedulability-proof lineage (Coq/Rocq-verified) is the reusable
  groundwork. → gates the "tri-track verified core" claim.
- **T2 — Object-code verification / tool-qualification.** Drive synth **RULE-VERIFY (ASIL-D)**
  coverage to **zero-gap on the shipped trusted-core object** (the v0.5.0/v0.6.0 switch/MPU/HM
  binary), closing the source→binary gap (§5). synth#757 coverage is the first milestone
  (gates v0.4.0). → gates every *partition-isolation* claim.
- **T3 — spar timing bridge (machine-checked schedulability).** Extend spar to derive the
  standard periodic-resource supply-bound blackout `2(Π−Θ)` as the inner tasks' release-jitter, feed the
  existing **Lean-verified** jittered WCRT recurrence, add the cooperative non-preemptive
  blocking term, cap demand against `sbf` → spar emits a **machine-checked inner-response
  bound** (stronger than DWT). → gates every *bounded-latency* claim (v0.6.0+).
- **T4 — Sound static-WCET inputs (vertical, gale-owned).** The WCRT analysis (T3) needs a
  *sound* per-function cycle bound as input — and on the BUILD/all-from-us path the natural
  owner is **synth itself**: synth emits the exact instructions, so synth can emit a sound
  per-function timing bound from its own model (a gale-owned static-WCET, not a third-party
  WCET tool). This keeps the "no outside" thesis intact for timing too. A recognized rigorous
  complement (a widely-used measurement-based method) is **statistical / probabilistic WCET
  (MBPTA / Extreme-Value-Theory)** —
  measurement-based but sound-by-EVT, categorically above raw DWT high-water-mark; gale may
  adopt it to cross-check the synth-emitted static bound. **No partition budget may be
  sized from DWT** (build-gate). → gates every DAL-A/ASIL-D *timing* claim.

## 3.1 Industry benchmark & the runtime-protection question

The relevant industry comparators (certified safety RTOSes and the emerging safe-async-Rust
runtimes) were surveyed; the sources are kept in internal notes. The lessons that shape this
plan — none of which require naming a third party:

- **Industry earns ASIL-D at the tool-qualification + runtime-enforcement layer, not by
  proving the scheduler correct** (ISO 26262-8:11 process qualification + belt-and-suspenders
  runtime monitors: execution-budget / arrival-rate / lock-budget enforcement + an independent
  watchdog). An assessor will therefore **expect a runtime-protection analogue** from gale, or
  an explicit, defensible argument for why gale's build-time proofs make one *provably*
  unnecessary. gale already has the analogues — the outer switch's **non-maskable window-end
  preemption + budget monitoring** (temporal), **I-ISO's MPU/MMU boundary** (spatial), and the
  **F100 HM + wdg-thin IWDG** (independent backstop) — and this plan should present them *as*
  the runtime-protection story (timing-protection maps to the budget monitor; the watchdog to
  the F100/IWDG). gale's differentiator: these are **machine-proven and their codegen
  object-code-verified**, where the industry norm is process-qualified.
- **The async-specific gap is industry-wide — and is gale's opening.** No safe-async-Rust
  runtime or certified async scheduler surveyed carries a *proof* of the WCET at every
  `.await`/poll point; that one piece of the timing safety case is argued empirically, or
  punted, everywhere. gale's dissolve-to-native + machine-checked WCET-at-poll (T3/T4) +
  no-alloc/no-panic is, on the current survey, **unattempted elsewhere** — the clearest
  differentiator in the plan.
- **Framing:** gale's proofs land best as **evidence feeding a tool-confidence-level (TCL)
  argument** (ISO 26262-8:11 / DO-330), not as a categorical "we don't need qualification."
  The checkable-certificate thesis (§0.2) *is* a tool-confidence argument — present it so.

(Comparator details and sources are held in internal research notes, out of this public plan,
per the no-third-party-names rule.)

## 4. Definition of done (per release) & the honesty gate

A release is done when: (1) its rivet requirement(s) are `verified` with the V closed
(requirement → architecture → implementation → oracle); (2) its oracle gate is mechanically
green (not a prose review); (3) its two-column ledger entry states, in the same breath, what
assurance advanced **and** what is still not claimed; and (4) the release notes carry the
falsification statement for the new behaviour. Dissolves ≠ verified: every dissolved artifact
is probe- or Kani-gated before it counts.

## 5. Critical path & the one keystone

`v0.4.0 (seam)` → **`v0.5.0 (I-ISO core — the keystone)`** → `v0.6.0 (outer switch)` →
`v0.7.0 (value-domain HM)` → `v1.0 (safety case)`, with T1–T4 threading across. **v0.5.0 is
the keystone:** it is the first release to deliver *any* isolation evidence and the one that
turns I-ISO from a documented invariant into a demonstrated, verified, hardware-enforced
boundary (with the synth#757 containment demo as its flagship acceptance test). Everything
after it composes; nothing before it carries a safety claim.

## 6. Open items — dispositions (user review, 2026-07-16)

- **Per-target certification level(s) — DEFERRED by decision:** stays a per-deployment
  output (spec §12), decided when a deployment lands; not a gate on the line. The
  methodology bar (rivet safety-case discipline, checkable certificates) is maintained
  regardless.
- **T1–T4 sequencing — resolved by execution:** the de facto order (T1 certificate pilots
  continuous; T2 riding synth's coverage curve; T3/T4 filed upstream and gating the timing
  claims from v0.6.0 on) is ratified as planned.
- **RT1176 interference characterization owner + TCM-vs-XIP — open** (no RT1176 BSP yet;
  revisit when the BSP work starts).
- **v0.5.0 flagship as public artifact — YES by decision:** the synth#757 containment demo
  (a real compiler miscompile physically denied by the verified MPU core) becomes the
  public showcase for the "contain-the-compiler" thesis; a Pages/showcase write-up is
  queued as follow-on work.
