# gust timing tracks (T3 / T4) — upstream extension specs

**Status:** specs filed upstream, 2026-07-16. **Context:** the gust safety release line
(`docs/superpowers/plans/2026-07-15-gust-safety-release-line.md` §3 — plan lands
separately via `plan/gust-safety-release-line`; the path does not exist on `main` until
that branch merges) carries two
cross-cutting timing tracks that gate every *bounded-latency* claim (v0.6.0+) and every
DAL-A/ASIL-D *timing* claim. Both are extensions of our own tools; the precise
specifications now live as upstream issues:

| track | rivet requirement | upstream spec | what it delivers |
|---|---|---|---|
| **T3** — machine-checked schedulability bridge | `REQ-OS-SCHED-001` | [pulseengine/spar#331](https://github.com/pulseengine/spar/issues/331) | spar derives the partition blackout `2(Π−Θ)` as inner release jitter, feeds its **existing Lean-verified jittered WCRT recurrence** (`RTAJittered.lean`, 0 `sorry`) unchanged, adds the cooperative non-preemptive blocking term `B_i = max` lower-priority poll-segment WCET, caps demand against the supply-bound function `sbf(t) = (Θ/Π)(t − 2(Π−Θ))` (the fallback demand test carries any per-task extra release jitter — or, if scoped out in v1, that combination is declined with the same hard Error, never silently under-approximated), and rejects unschedulable sets loudly → a **machine-checked inner response-time bound** analyzed against partition supply, not wall-clock. |
| **T4** — sound static-WCET inputs | `REQ-OS-WCET-001` | [pulseengine/synth#778](https://github.com/pulseengine/synth/issues/778) | synth emits a **sound per-function worst-case cycle bound** alongside the object (Cortex-M3 first): worst-case path over the CFG × a validated per-instruction timing table, loop bounds from annotations or the verified `decreases` measure, loud decline on unbounded constructs, sidecar output consumed by T3 as `C_i`. Measurement validates the timing **model**, never becomes the budget. |

The rivet artifacts (`REQ-OS-SCHED-001`, `REQ-OS-WCET-001` and their verification
counterparts) land separately through the feature loop; this doc is the tracking link
between the release plan, the requirement IDs, and the upstream specs.

## The chain (why this beats measurement)

```
synth cycle bound (T4)          — sound C_i per poll segment, from the emitted
                                  instructions themselves; no third-party analyzer
        ×
Verus-proven poll structure     — poll_round loop machine-checked terminating
  (src/executor.rs)               (decreases ready_popcount(), ≤ MAX_TASKS = 8);
                                  exactly-once dispatch follows from the proven
                                  strictly-decreasing popcount + no-re-set framing;
                                  run-to-completion
        ×
spar Lean-verified composition  — supply-derived jitter 2(Π−Θ) + blocking term into the
  (T3)                            0-sorry jittered fixed-point recurrence, capped by sbf
        =
a machine-checked response-time bound with NO MEASUREMENT anywhere in the chain.
```

Cycle counters (DWT high-water-marks) appear in exactly one place: falsifying the T4
timing model on a golden function set (`bound ≥ observed`, every run). They never feed a
budget, a `C_i`, or a bound. **No partition budget may be sized from DWT** — the release
line's build-gate.

## Honesty caveats (carried from the specs)

- The T3 bound is only as sound as its `C_i`/`Θ` inputs — hence T4 gates T3's use in the
  safety case; neither track stands alone.
- Unqualified provers (Lean, Verus) carry no tool-qualification credit by themselves; the
  machine-checked bounds are rigor and safety-case inputs feeding a tool-confidence
  argument, not certification claims.
- T4 is additive tooling on a maturing compiler, scoped to the trusted-core code class
  (straight-line + bounded loops, no dyn/alloc/recursion, near-deterministic ARMv7-M
  timing) — not a general-purpose WCET analyzer and not a qualification claim.
