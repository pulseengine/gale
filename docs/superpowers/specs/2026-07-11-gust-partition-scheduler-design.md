# gust two-level partition scheduler + tiered supervision — design

**Status:** design (validated with jess on gale#63; awaiting user review before an
implementation plan).
**Date:** 2026-07-11. **Revised:** 2026-07-15 (rev 3 — added the silicon-independence
governing principle §1.1; rev 2 was the six-lens expert-review pass — see the revision
note below).
**Origin:** user ask ("kiln async should do more — priority tasks + supervision"), the
ASIL-D / actually-lands reframe, a verified-scheduling research survey, and jess's
gale#63 endorsement + 3-core RT1176 mapping.

> **Revision note (rev 3).** Added §1.1 as the *governing principle*: the silicon is a
> substitutable external parameter, not a premise — the design is silicon-independent by
> construction (author once in the Component Model, dissolve to any synth target, isolate
> via that target's MPU/PMP), and the certification level is a per-target output bounded by
> the silicon's physical ceiling, never a gate on the general argument. §11 (RT1176
> interference) and §12 (cert target) are reframed accordingly: they *parameterize* the
> argument per deployment; they do not dominate it. This resolves the review panels'
> largest tension (whether the RT1176 interference wall is a design-killer) by scoping it
> to the RT1176 *deployment*, with graceful degradation to a lower assurance level (or a
> silicon change) where hardware is insufficient.
>
> **Revision note (rev 2).** A six-perspective expert review (WASM Component Model,
> functional-safety certification, real-time/hierarchical scheduling, drone flight-SW
> integration, formal methods, embedded security) found the architecture shape sound but
> the *isolation story* systematically over-credited to the CM/dissolve layer, which
> cannot carry it after `meld --memory shared` fusion. Rev 2 makes the **hardware MPU the
> explicit isolation root of trust** (§2.1), aligns every claim with what a fused,
> dissolved image can actually guarantee, reframes the Verus proofs as functional/liveness
> (not timing) evidence, resolves two internal contradictions (ports-vs-locks,
> fused-component-vs-process-restart), and promotes the certification target, the RT1176
> multicore-interference plan, and value-domain health monitoring from open items to
> first-class obligations.

## 1. Problem & goal

gust's async today is `gust:os/spawn` — cooperative, scalar, dissolved-to-native
(`gust_poll` = kiln `poll_round`). Cooperative-only gives no bounded latency and no
freedom-from-interference when a task faults or over-runs. For a flight target you land
(Pixhawk 6X-RT / i.MX RT1176), the scheduling **backbone** must be certifiable. Goal:
add priority scheduling + fault supervision **without inventing a novel scheduler** —
keep the proven backbone, make what's new be its *size*, its *verification*, and the
ergonomics that ride on top.

### 1.1 Silicon is a substitutable external parameter (governing principle)

The architecture is **silicon-independent by construction**, and this — not any one chip —
is what the general argument rests on. A component is authored once in the WebAssembly
Component Model, dissolved by synth to whatever native target is in hand (cortex-m3 / m4 /
m7 and riscv today — all three already anchored on real silicon; more as backends land),
and its spatial isolation is rooted in *that target's* hardware memory-protection unit
(ARMv7-M / ARMv8-M **MPU** or RISC-V **PMP**) per I-ISO (§2.1). The silicon is therefore an
**input parameter, not a premise**: it *bounds* the assurance level and timing margin a
given deployment can claim, but it neither validates nor invalidates the design.

- **Today = i.MX RT1176**, because it is the best Pixhawk-class silicon currently
  available. The design assumes this will change and does not hard-code to it.
- **Better silicon → supported.** Retarget the synth backend + the MPU/PMP region table;
  the verified core and its proofs are written silicon-parametrically (mmio addresses +
  region layout are configuration, not code). Porting is a re-instantiation, not a redesign.
- **Insufficient silicon → graceful degradation, not failure.** Where a target physically
  cannot meet a safety bar (e.g. DAL-A temporal isolation on a shared-XIP, shared-bus part
  — see §11), the *deployment* is capped at the level that silicon supports (DAL-C / SORA
  specific-category, or the DAL-A-critical slice relocated to TCM). That is an **external
  hardware limitation, explicitly not a flaw in the architecture.**

**Consequences for the rest of this document (so nothing downstream reads a silicon fact
as a design verdict):** the certification *level* (§12) is a **per-target output**, a
function of the integrator's need *and* the silicon's physical ceiling — not a precondition
that gates the architecture; the general argument targets *the highest assurance the target
silicon physically permits*, with the degradation ladder above. The RT1176 multicore-
interference analysis (§11) is a **per-target capability bound on the RT1176 deployment**,
not a blocker on the design. Neither dominates the argument; both parameterize it.

## 2. The two-level model (endorsed by jess, research-confirmed canonical IMA)

- **Outer — fixed-priority, time-partitioned, preemptive** (ARINC 653 major-frame /
  AUTOSAR-OS timing-protection). A trusted timer-tick does a partition switch; each
  partition gets a guaranteed, offline-planned time window. **Temporal** isolation comes
  from the non-maskable window-end preemption; **spatial** isolation comes from the MPU
  region-swap on switch (§2.1) — *not* from the Component Model. This is the certifiable
  "no other way" backbone.
- **Inner — cooperative kiln-async inside a partition's window.** The dissolved async
  tasks (`poll_round`) run within one partition's window — where the "superior
  experience" (zero-alloc, dissolved, ergonomic async) lives, made safe by being boxed in
  a preemptive partition. Note the ergonomics are an *intra-partition structuring* benefit;
  they are explicitly **not** an isolation mechanism (§2.1).

**The load-bearing safety property (jess underlined it):** safety lives in the **outer
timer + Health Monitor + MPU, not in inner good behaviour.** A non-yielding cooperative
task is contained *temporally* by the **non-maskable window-end preemption** and
*spatially* by the MPU — both of which MUST live in the verified core and be un-maskable
by inner code. The survey confirmed "cooperative scheduling inside a bounded preemptive
window" is an accepted composable-scheduling pattern; the assessor's burden for the
*isolation* argument is bounding the window **and** proving the spatial boundary — it is
**not** discharged by the inner discipline (see §7 on why inner *schedulability* is a
separate, still-open obligation).

### 2.1 Isolation root of trust — the load-bearing architectural invariant (rev 2)

**INVARIANT (I-ISO):** *The hardware MPU (ARMv7-M PMSA) is the sole root of trust for
spatial partition isolation. The Component Model, WIT capabilities, and the dissolve
pipeline provide ergonomics and intra-partition structure only. A synth/meld/loom
miscompile, a wasm bounds-check elision, or a corrupted inner component can corrupt
memory only **within** its own partition — never across a partition boundary — because
the MPU, programmed on every partition switch by the verified core, physically denies the
access.*

Why this invariant, not the CM story: `meld --memory shared` fuses an app + its OS
providers into **one** linear memory and the dissolve removes the runtime, the
Canonical-ABI copy boundary, and inter-instance memory disjointness. WebAssembly's SFI
protects the *host* from a module; it provides **no** protection between regions
co-resident in a single linear memory, and gale's proof-carrying clamp-elision removes
even the intra-memory bounds checks. Therefore "capability-isolated" is a **compile-time
type property, not a runtime boundary.** Freedom-from-interference (ISO 26262-6) and
robust partitioning (DO-178C §2.4) require I-ISO's hardware boundary; they cannot be met
by the fused/dissolved layer alone.

Consequence for the TCB (see §5): I-ISO is precisely what keeps the compiler out of the
*isolation* TCB — any codegen defect is contained within a partition. Until the MPU
region-swap mechanism exists and is *demonstrated* to contain a compiler-introduced
breach, **no** freedom-from-interference or partition-isolation claim attaches to any
buildable artifact, and the fused syscall path (v0.4.0) carries none.

## 3. RT1176 partition mapping (jess owns the OS/silicon side)

| core | outer partition(s) (ARINC 653) | inner (kiln-async) | role |
|---|---|---|---|
| **M7** (fpv5-d16) | flight-control window (~1 kHz) **+ a mission/payload window** | falcon cascade (flight); MAVLink/log/nav (mission) | flight + mission partitions |
| **M4** (fpv4-sp) | estimator window | IEKF, one fused async component | estimation partition |
| **F100** (M3, 8 KB) | — | gust failsafe (fixed-point, DD-011) | **cross-core Health Monitor** |

Each partition hosts one fused async component; **a core may host more than one
partition.** Rev 2 makes this explicit because a strict one-partition-per-core mapping
makes the ARINC 653 major-frame degenerate (partition-vs-idle = single-partition budget
monitoring, not time-partitioning) and leaves nowhere for the mission/payload half of a
real drone (MAVLink, logging, navigation/mission executor, GCS link, parameter storage).
**Open (promoted, §10):** the major-frame (LCM of partition periods) construction, window
sizing, and inter-partition schedulability for the multi-partition-per-core case.

## 4. Tiered Health Monitor (the "both, tiered" supervision — three tiers)

The tiered supervision the user chose maps onto the ARINC 653 Health Monitor as a
**table-driven, three-tier error-response mechanism** (not a heartbeat alone):

1. **Per-core (tier 1):** the 653 window-end preemption on M7/M4 + AUTOSAR-style budget
   monitoring. Within a partition: process-level **restart of a *non-critical* faulting
   task** (availability), bounded by a restart-count + cooldown, escalating to partition
   fail-to-safe on repeated fault / budget-overrun / deadline-miss. **The flight-critical
   path (attitude/rate control) is fail-over-only, never restarted in flight** — a mid-air
   restart of the controller is loss-of-control, not recovery (rev 2 fix).
2. **Cross-core (tier 2):** the **physically independent F100** running gust is the
   ultimate spatial+temporal containment for the two FMU cores. It consumes the M7↔M4 MU
   heartbeat (gale#65 doorbell datapath, proven in the 3-core Renode model) and trips the
   4× failsafe-PWM on FMU loss / window overrun. Faithfully mirrors the proven Pixhawk
   FMU/IOMCU failsafe split.
3. **Module (tier 0 backstop):** the **wdg-thin IWDG driver** (cannot-un-start,
   Kani-proven) as the hardware fail-to-safe if a HM core itself hangs.

### 4.1 Value-domain monitoring (rev 2 — was missing)

Tiers 1–3 above are **liveness/timing** monitors (heartbeat-loss, window-overrun,
budget-overrun, deadline-miss). The dominant real drone loss mode is **erroneous-but-timely
output**: a task meets its deadline while emitting garbage (EKF/estimator divergence,
stale or implausible sensor data, controller saturation). A liveness monitor is
structurally blind to this. Rev 2 requires, as part of the HM safety argument:

- **Value-domain checks** — data-freshness/staleness gates, innovation/plausibility
  gating, sensor voting, estimator-divergence detection (the checks PX4/ArduPilot use, not
  watchdogs).
- **An explicit failsafe state machine** — RC-loss, datalink-loss, GPS/estimator-loss,
  geofence breach, low-battery — each mapping onto the F100 tier-2 trip.
- **A stated diagnostic-coverage figure** against a named fault model (ASIL-D expects an
  argued ~99% DC that liveness alone cannot supply).

Escalation is provably terminating (bounded restart-count → partition fail-to-safe →
cross-core F100 trip); the HM *policy* is a small FSM verified with Kani/Verus. (Note: the
policy FSM being verified does **not** verify the value-domain *detectors* — those carry
their own analysis.)

## 5. The verified TCB boundary (rev 2 — scoped precisely)

- **Trusted + proven (small):** the partition-switch + **MPU-program-on-switch** + HM
  core — an mmio + scalar-FSM sliver with no heap, matching the thin-seam driver TCB
  discipline. Precedent for a *small* verified isolation core: seL4-MCS (functional
  correctness proven), Muen (SPARK), ProvenCore. (seL4-MCS is cited **directionally** for
  the "time as a budget capability" *shape* only; its proofs are for a *dynamic* MCS
  model on an MMU base and do **not** transfer to static ARINC 653 windows — verified 653
  temporal partitioning is a separate ~76k-LoC effort.)
- **Untrusted / dissolved (outside the core):** apps, drivers, the async machinery — all
  dissolved capability-components, contained by I-ISO's MPU boundary.
- **synth's TCB position (corrected):** synth is out of the **app-correctness** TCB (apps
  are dissolved-untrusted). But (a) the trusted switch/HM/MPU core must itself be compiled
  to native by *some* backend, so that backend is in the **flight TCB**; and (b) a
  miscompile of a cross-partition copy or handle *is a partition breach*, so absent I-ISO
  the pipeline would be in the **isolation TCB** too. **synth#757** (a static-string buffer
  copy that reads the wrong source bytes on the fused syscall path) is a live existence
  proof of this class. Closure plan for the *trusted core's* source→binary gap: translation
  validation (seL4/Valex-style), a DO-330-qualified compiler, or object-code proof — the
  project's gale#173 LRAT direction (trust the checker, not the solver) is the natural
  route. I-ISO is what bounds the blast radius of any residual defect to within a partition.

## 6. Honesty constraints & non-goals (from the research survey)

These are hard lines, not preferences:

- **DWT ≠ certification-grade WCET.** DWT cycle counts (our 1.448×/1.839× silicon anchors)
  are *measurement-based* → **high-water-mark**, and unsound on multicore. DO-178C/DO-333
  want a *sound* bound (static analysis). Our DWT numbers are **evidence + regression
  guards**; DAL-A partition budgets need static WCET (or hybrid + provable path coverage)
  + margin, stated explicitly. **No downstream doc may size a partition budget from a DWT
  number.** (Discipline: verification-claims-honesty.)
- **Migration stays strictly below the safety line.** Checkpoint/restart + task migration
  (the kiln#415 interpreted-tenant rung) are availability-only / non-critical /
  redundant-lane; the safety function recovers by **local fail-to-safe**, and redundancy
  is *static* (the independent F100), never dynamic migration on the critical path.
- **Reject Vestal-style mixed-criticality dropping.** Deployable MC = static partition
  isolation, not "drop low-crit on a mode switch." Criticalities live in separate
  partitions; inner tenants share a partition only at a shared assurance level.
- **Verified ≠ timing-verified, and verified ≠ certified.** Prove functional correctness of
  the switch/HM core mechanically; treat WCET/timing as a *separate, partly-manual*
  soundness case (seL4's own gap). Also: unqualified Verus/Z3, Kani/CBMC, and Rocq carry
  **zero DO-330 tool-qualification credit** on their own — the "verified TCB" is
  engineering rigor and an input to a safety case, **not** certification evidence until the
  tools are qualified or the results independently checked.
- **Inter-partition channels use ports, never shared locks (rev 2).** Cross-partition
  communication is spar-generated WIT **sampling/queuing ports** (§9). Priority-ceiling
  protocols (**PCP/ICPP**, immediate ceiling, deadlock-free, ceiling proven ≥ max user
  priority) are used **strictly intra-partition**. A cross-partition ceiling lock would
  make one partition able to suspend partition switching and defeat temporal isolation
  (and is a timing covert channel) — real IMA forbids inter-partition shared locks.
  (Correction: a *protocol* is not "certified"; a specific *implementation* is
  qualified/verified.)
- **Multicore/shared-bus temporal interference on RT1176 is a foundational blocker, not a
  footnote (rev 2 → §11).**

## 7. The gale-side critical path: the executor (epic #3) — SHIPPED, with scope

jess's one open dependency on our side was the inner **async executor** — Embassy-class,
`no_std`, dissolved. **Status: built, verified, and merged (PR #176).** Verus obligations
discharged (1081/0), Kani 2/2, dissolved to a single cortex-m3 object and qemu-probed:

- **no-lost-wakeups** — a set ready-bit is never dropped.
- **bounded-poll** — each `poll_round` visits each ready task at most once; terminates
  (decreases on ready-popcount).
- **fair-ready-queue** — highest-priority-ready runs; work-conserving; no starvation
  within a partition window.

**These are FUNCTIONAL / LIVENESS properties, not timing credentials (rev 2, load-bearing
clarification).** `bounded-poll` is a **step-count / termination** bound — it is **not** a
wall-clock latency bound (DO-178C 6.3.4f WCET is wall-clock). With DWT disclaimed and a
static-WCET toolchain not yet selected, the **intra-partition response time** of a
high-priority inner task released mid-window is **not yet bounded**. Two further open
obligations that the functional proofs do *not* cover:

- **Compositional schedulability (open, §10).** The tickless inner executor must be
  analyzed against the outer partition's **supply-bound function** (periodic resource
  model, Shin & Lee), *not* wall-clock. A tickless `Timer::after(200µs)` that expires while
  the partition is descheduled is not observed until resume, so inner release jitter is up
  to (period − budget) — e.g. ~700µs on a 1 kHz / 300µs-budget flight window. The spec
  must state whether inner deadlines are partition-local or global time and bound the
  effective-deadline degradation.
- **The `poll_task` FFI seam is an unbounded hole the proofs cannot see across.**
  bounded-poll termination is valid only *assuming* each callee returns, does not re-enter,
  and does not mutate the ready-queue. Kani/CBMC give no guarantee past the FFI. Rev 2
  requires an explicit **callee contract** (counted in the TCB) and an **I/O-quiescent-point
  requirement** so a non-maskable window-end preemption cannot fire mid-MMIO-transaction
  inside a thin-seam driver and hang a peripheral on its own bus timeout (or a
  partition-restart recovery for that case).

**MAX_TASKS ≤ 8 must be an *enforced* runtime invariant**, not just a Kani domain bound —
Kani is bounded model checking, so "Kani 7/7 at N=8" proves nothing about a 9th task or v2
dynamic spawn unless the trusted core mechanically rejects admission beyond the bound.

### 7.1 Tri-track verification map (rev 2 — was asserted, now itemized)

"Satisfy Verus + Rocq + Kani" buys diverse-redundancy **only if all three discharge the
same load-bearing theorem.** Honest current status:

| property | Verus | Kani | Rocq |
|---|---|---|---|
| no-lost-wakeups | ✅ proven | ✅ (bounded, N≤8) | ⬜ not yet (Rocq track has stubs) |
| bounded-poll (termination) | ✅ proven | ✅ (bounded) | ⬜ |
| fair / work-conserving pick_next | ✅ proven | ✅ (bounded) | ⬜ |
| tickless next_deadline/expire | ✅ proven | — | ⬜ |
| MPU-program-on-switch correctness | ⬜ (unbuilt) | ⬜ | ⬜ |

The Rocq column is currently empty (project memory flags Rocq stubs); the "tri-track"
claim is **aspirational** until at least one load-bearing theorem is discharged on all
three, and none of the three yet carries qualification credit (§6).

## 8. Shippable increments — and what each does / does NOT assure

- **v1 (shipped inner executor + HM FSM):** the cooperative async executor in one fixed
  window + the verified HM *policy* FSM + the wdg-thin HW backstop. **Assurance honesty
  (rev 2): v1 delivers ZERO freedom-from-interference.** It is a functional prototype of
  the explicitly-non-safety inner layer plus an availability-tier HM FSM — there is no
  partition isolation (no outer preemptive switch, no MPU region-swap), no bounded
  intra-partition latency, and the fused syscall path is blocked on synth#757. The inner
  executor being merged (PR #176) does **not** advance the safety case; by this spec's own
  logic safety lives in the still-unbuilt outer layer.
- **v2 (the safety-load-bearing epic — mostly UNBUILT):** the outer preemptive
  time-partitioning + the **MPU region-swap-on-switch** (the mechanism that *discharges*
  I-ISO) + the verified syscall/TCB boundary + multi-partition scheduling. This is where
  freedom-from-interference is earned. Gated on: the MPU mechanism (verified obligation),
  the synth#757 fix + the trusted-core source→binary closure (§5), and meld's reloc-consumer
  for the fused multi-provider node (gale-side `--emit-relocs` already solved, gale#168).
- **v3:** the full multi-core mapping in the Renode model (jess wires the 653 outer window +
  kiln inner; the F100/gust HM tier is already demonstrable), with the multicore-interference
  mitigation (§11) in place.

## 9. Traceability & toolchain fit (feature-loop step 1 = architecture in spar)

- **spar:** jess models the ARINC 653 partitioning **and the MPU region layout** in
  `hardware/pixhawk6x-rt.aadl` — partition windows + region table + Health Monitor — and
  runs spar's ARINC 653 partitioning + modal-scheduling analysis. Inter-partition
  interfaces are spar-generated WIT **sampling/queuing ports** (REQ-PIX-007:
  `wit/{falcon,estimator,failsafe}.wit`) — never shared locks (§6).
- **rivet:** the two-level model, I-ISO, the HM policy + value-domain coverage, the
  DWT-vs-static-WCET honesty boundary, and the certification target as typed
  requirements/decisions; ASIL-D/DAL decomposition. A jess DD records the two-level model,
  I-ISO (MPU-as-root), and F100-as-cross-core-HM mapping.
- **Renode:** the 3-core model (gale#65 MU doorbell) is the proving ground; the F100/gust
  HM tier is already demonstrable.

## 10. Threat model & security (rev 2 — was absent)

The design is positioned as "multi-tenant" on a target with an SE051 (EAL6+), so it must
state an adversary model: freedom-from-interference (against *random* faults) and a
*security* boundary (against a crafted-input adversary) are different obligations
(STPA-Sec / Common Criteria require the model stated).

- **Stated model (to be confirmed by jess):** tenants within a partition are mutually
  *trusting* at a shared assurance level; partitions are mutually *distrusting* for
  *safety* (freedom-from-interference) but v1/v2 do **not** yet claim a *security* boundary
  between partitions.
- **If "multi-tenant" means mutually-distrusting for security**, add: secure/measured boot
  rooted in the **SE051** (a tampered fused image or F100 image otherwise silently defeats
  every proof and tier-2), attestation, and **cache/prefetch scrubbing on partition switch**
  (L1 + FlexSPI prefetch are cross-partition micro-architectural side/covert channels — the
  confidentiality analogue of §11).

## 11. RT1176 multicore-interference — a per-target capability bound (not a design blocker)

Per §1.1, this section bounds the *RT1176 deployment's* claimable assurance level; it does
not gate the architecture. M7@1GHz + M4@400MHz share the AXI bus and the 64 MB XIP flash;
code executes from external QSPI. A memory-bound M4 task stalls M7 instruction fetch, so the
M7 partition's WCET is **not independent** of M4 — the temporal-isolation premise fails on
the naive layout, and the literature documents 8–13× WCET inflation from unmitigated
contention. The independent F100 detects loss-of-function; it **cannot** restore a missed
deadline or bound interference-induced inflation. So on *this silicon*: the mitigations
below determine whether the RT1176 deployment claims DAL-A (critical slice TCM-resident +
bandwidth control) or degrades to DAL-C/ASIL-B/SORA — and if even the mitigated ceiling is
too low for the integrator's need, that is an RT1176 limitation answered by picking better
silicon, not a defect in the general design. Required for the RT1176 target (CAST-32A /
AMC 20-193):

- Relocate DAL-critical code/data to **ITCM/DTCM** (not XIP).
- Enforced **bandwidth/arbitration control** (or way-partitioning) between M7 and M4.
- The CAST-32A **interference-channel enumeration** (bus, flash controller, shared cache,
  DMA) and a per-channel mitigation/characterization.

## 12. Open items (owners + gates)

- **Certification target — a PER-TARGET OUTPUT, not a gate on the architecture (§1.1).**
  For each deployment: the claimable DAL letter / ISO 26262 ASIL / SORA specific-category is
  the *min* of (the integrator's need, the silicon's physical ceiling from §11). jess owns
  fixing it *per target*; the general argument does not wait on it. What it sets per target:
  the WCET-justification bar and whether that silicon's ceiling meets the mission — if not,
  the answer is better silicon or a lower-assurance deployment, not a redesign.
- **MPU region-swap mechanism + fused-memory layout / region-budget plan.** ARMv7-M PMSA
  gives ~8–16 power-of-2-sized, base-aligned regions; N-tenant isolation in one linear
  memory is capped by that budget and alignment. This is where I-ISO is *discharged*.
- **synth#757 fix + trusted-core translation-validation/qualification route (§5).** Gates
  the v0.4.0 fused syscall seam; until then the fused path carries no isolation claim.
- **Static-WCET toolchain (aiT / RapiTime / hybrid) + compositional supply-bound analysis
  (§7).** Gates DAL-A budget and inner-deadline claims.
- **meld copy-vs-reference semantics for `--memory shared`.** If it passes references into
  the shared arena instead of Canonical-ABI copies, it opens a TOCTOU / shared-mutable-state
  channel *before* the MPU question arises. Copy = isolation; reference = speed. Must be
  pinned.
- **Value-domain HM detectors + failsafe state machine + diagnostic-coverage claim (§4.1).**

## 13. Non-goals

Not a novel scheduling algorithm; not a full RTOS; not dynamic migration on the critical
path; not a claim that DWT gives certification WCET; **not a claim that the CM/dissolve
layer provides partition isolation** (the MPU does, per I-ISO); **not a security boundary
between partitions in v1/v2** (see §10). This design keeps the certifiable ARINC 653
backbone and contributes the small verified core (switch + MPU-program + HM) + the
dissolved cooperative inner layer.
