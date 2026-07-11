# gust two-level partition scheduler + tiered supervision — design

**Status:** design (validated with jess on gale#63; awaiting user review before an
implementation plan).
**Date:** 2026-07-11.
**Origin:** user ask ("kiln async should do more — priority tasks + supervision"), the
ASIL-D / actually-lands reframe, a verified-scheduling research survey, and jess's
gale#63 endorsement + 3-core RT1176 mapping.

## 1. Problem & goal

gust's async today is `gust:os/spawn` — cooperative, scalar, dissolved-to-native
(`gust_poll` = kiln `poll_round`). Cooperative-only gives no bounded latency and no
freedom-from-interference when a task faults or over-runs. For a flight target you land
(Pixhawk 6X-RT / i.MX RT1176), the scheduling **backbone** must be certifiable. Goal:
add priority scheduling + fault supervision **without inventing a novel scheduler** —
keep the proven backbone, make what's new be its *size*, its *verification*, and the
ergonomics that ride on top.

## 2. The two-level model (endorsed by jess, research-confirmed canonical IMA)

- **Outer — fixed-priority, time-partitioned, preemptive** (ARINC 653 major-frame /
  AUTOSAR-OS timing-protection). A trusted timer-tick does a partition switch; each
  partition gets a guaranteed, offline-planned time window; temporal + spatial isolation
  enforced. This is the certifiable "no other way" backbone.
- **Inner — cooperative kiln-async inside a partition's window.** The dissolved async
  tasks (`poll_round`) run within one partition's window — where the "superior
  experience" (zero-alloc, dissolved, capability-isolated ergonomic async) lives, made
  safe by being boxed in a preemptive partition.

**The load-bearing safety property (jess underlined it):** safety lives in the **outer
timer + Health Monitor, not in inner good behaviour.** A non-yielding cooperative task
is contained *by construction* by the **non-maskable window-end preemption** — which
therefore MUST live in the verified core and be un-maskable by inner code. The survey
confirmed "cooperative scheduling inside a bounded preemptive window" is an explicitly
accepted composable-scheduling pattern; the assessor's burden is *bounding the window*,
not the inner discipline.

## 3. 3-core RT1176 partition mapping (jess owns the OS/silicon side)

| core | outer partition (ARINC 653) | inner (kiln-async) | role |
|---|---|---|---|
| **M7** (fpv5-d16) | flight-control window (~1 kHz) | falcon cascade, one fused async component | flight partition |
| **M4** (fpv4-sp) | estimator window | IEKF, one fused async component | estimation partition |
| **F100** (M3, 8 KB) | — | gust failsafe (fixed-point, DD-011) | **cross-core Health Monitor** |

Each partition hosts exactly one fused async component (jess's epic model: one fused
component per thread/partition, multiple across cores).

## 4. Two-tier Health Monitor (the "both, tiered" supervision)

The tiered supervision the user chose maps onto the ARINC 653 Health Monitor, and jess's
mapping makes it **two physical tiers**:

1. **Per-core (tier 1):** the 653 window-end preemption on M7/M4 + AUTOSAR-style budget
   monitoring. Within a partition: process-level restart of a faulting inner task
   (availability), bounded by a restart-count + cooldown, escalating to partition
   fail-to-safe on repeated fault / budget-overrun / deadline-miss.
2. **Cross-core (tier 2):** the **physically independent F100** running gust is the
   ultimate spatial+temporal containment for the two FMU cores. It already consumes the
   M7↔M4 MU heartbeat (gale#65 doorbell datapath, proven in the 3-core Renode model) and
   trips the 4× failsafe-PWM on FMU loss / window overrun. Stronger than a single-core HM
   — it's the gust story we already have.
3. **Module (tier 0 backstop):** the **wdg-thin IWDG driver** (cannot-un-start,
   Kani-proven) as the hardware fail-to-safe if a HM core itself hangs (loss of service →
   HW reset).

Escalation is provably terminating (bounded restart-count → partition fail-to-safe →
cross-core F100 trip); the HM policy is a small FSM verified with Kani/Verus.

## 5. The verified TCB boundary

- **Trusted + proven (small):** the partition-switch + HM core — seL4-MCS
  *scheduling-context-capability* shape (time as a budget/capability), an mmio + scalar-FSM
  sliver with no heap, matching the thin-seam driver TCB discipline. Precedent for a
  *small* verified isolation core: seL4 MCS (functional correctness proven), Muen (SPARK),
  ProvenCore.
- **Untrusted / dissolved (outside the core):** apps, drivers, the async machinery — all
  dissolved capability-components. **synth stays out of the flight TCB.** The TCB grows
  from ~0 to "a small proven scheduler," still radically smaller than a conventional
  certified RTOS.

## 6. Honesty constraints & non-goals (from the research survey)

These are hard lines, not preferences:

- **DWT ≠ certification-grade WCET.** DWT cycle counts (our 1.448×/1.839× silicon anchors)
  are *measurement-based* → **high-water-mark**, and unsound on multicore. DO-178C/DO-333
  want a *sound* bound (static analysis). Our DWT numbers are **evidence + regression
  guards**; DAL-A partition budgets need static WCET (or hybrid + provable path coverage)
  + margin, stated explicitly. (Discipline: verification-claims-honesty.)
- **Migration stays strictly below the safety line.** Checkpoint/restart + task migration
  (the kiln#415 interpreted-tenant rung) are availability-only / non-critical /
  redundant-lane; the safety function recovers by **local fail-to-safe**, and redundancy
  is *static* (the independent F100), never dynamic migration on the critical path.
- **Reject Vestal-style mixed-criticality dropping.** Deployable MC = static partition
  isolation, not "drop low-crit on a mode switch." Criticalities live in separate
  partitions; inner tenants share a partition only at a shared assurance level.
- **Verified ≠ timing-verified.** Prove functional correctness of the switch/HM core
  mechanically; treat WCET/timing as a *separate, partly-manual* soundness case (seL4's
  own gap). Do not conflate the two.
- **New hazard to log:** multicore / shared-bus temporal interference on the RT1176
  (M7 + M4, shared bus + XIP) — outside every prior proof; needs its own safety-case entry
  and mitigation. (Partly answered by the independent F100 tier.)

Locks that cross the verified core / inter-partition channels use **PCP/ICPP** (immediate
priority ceiling; DO-178B-certified, deadlock-free), with the ceiling proven ≥ max user
priority — not plain PIP.

## 7. The gale-side critical path: the executor (epic #3)

jess's one open dependency on our side: the inner layer needs the **async executor** —
Embassy-class, `no_std`, dissolved — with **Verus** proof obligations:

- **no-lost-wakeups** — a set ready-bit is never dropped.
- **bounded-poll** — each `poll_round` visits each ready task at most once; terminates.
- **fair-ready-queue** — highest-priority-ready runs; work-conserving; no starvation
  within a partition window.

This is the concrete next gale deliverable that gates the whole inner layer. It composes
with epic #5 (AMP + the MU as cross-core transport) for the tier-2 HM datapath. It also
subsumes kiln#415 (the interpreter as the migratable-tenant rung — availability tier only).

## 8. Shippable increments

- **v1 (buildable NOW, unblocked lane):** a **static single-partition** slice — the
  cooperative async executor in one fixed window + the verified HM policy FSM (bounded
  restart → fail-to-safe) + the wdg-thin HW backstop. Single-component / stateless-read
  dissolve → **not meld#326-gated**. Kani/Verus-prove the executor + HM FSM, dissolve
  0-/bounded-SRAM, gate on Renode. This proves the inner layer + supervision without
  multi-partition preemption.
- **v2 (rides meld#326):** multi-partition preemptive time-partitioning + dynamic spawn.
  The gale-side reloc-core production is already solved (gale#168, `--emit-relocs`); v2
  waits on meld's reloc-consumer to fuse the multi-provider node cleanly.
- **v3:** the full 3-core mapping in the Renode model (jess wires the 653 outer window +
  kiln inner; the F100/gust HM tier is already demonstrable there).

## 9. Traceability & toolchain fit (feature-loop step 1 = architecture in spar)

- **spar:** jess models the ARINC 653 partitioning in `hardware/pixhawk6x-rt.aadl` —
  partition windows + Health Monitor — and runs spar's ARINC 653 partitioning +
  modal-scheduling analysis (adds the *temporal* layer the model was missing; the
  partition→core `Actual_Processor_Binding` is already there). Inter-partition interfaces
  stay spar-generated WIT (REQ-PIX-007: `wit/{falcon,estimator,failsafe}.wit`).
- **rivet:** the two-level model, the HM policy, and the DWT-vs-static-WCET honesty
  boundary as typed requirements/decisions; ASIL-D/DAL decomposition. A jess DD records
  the two-level model + F100-as-cross-core-HM mapping.
- **Renode:** the 3-core model (gale#65 MU doorbell) is the proving ground; the F100/gust
  HM tier is already demonstrable.

## 10. Open items

- Exact certification target (DAL letter / ISO 26262 ASIL-D) — jess owns; sets the WCET
  justification bar.
- Static-WCET toolchain choice for the DAL-A partition budgets (the DWT numbers don't
  suffice).
- The executor's Verus harness (no-lost-wakeups / bounded-poll / fair-ready-queue) — the
  first implementation artifact.
- Multicore/shared-bus interference mitigation on RT1176 — safety-case entry.

## 11. Non-goals

Not a novel scheduling algorithm; not a full RTOS; not dynamic migration on the critical
path; not a claim that DWT gives certification WCET. This design keeps the certifiable
ARINC 653 backbone and contributes the small verified core + the dissolved cooperative
inner layer.
