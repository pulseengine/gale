# T4 on the whole OS — 1 bound of 31, and the reasons are not the ones we planned for

**Date:** 2026-08-06, re-measured 2026-08-25 · **REQ-OS-WCET-001** · synth 0.52.0 → **0.57.0**, schema `synth-wcet-v1`,
core class `cortex-m3` · input: the E2 dissolved composite (31 functions)

E2 produced the first native object for the whole OS, which made T4 possible for the
first time. Running `synth --emit-wcet` over it gives the honest coverage number:

    BOUNDED    3  of 31
    DECLINED  28

The single bound is `gust:os/time@0.1.0#deadline` at **36 cycles**.

This is not a failure of the track — declines are loud and machine-readable by design,
which is the whole point of the sidecar. It is a measurement of how far T4 actually is
from feeding T3, and the answer is: further than the plan assumed.

## The 30 declines

| reason | n | what it means | whose problem |
|---|---|---|---|
| `loop` | **13** | backward branch without a statically-proven trip count | **scry** — loop-bound inference |
| `callee-unbounded` | 11 | a directly-called callee is itself unbounded | **cascade** — resolves when the leaves do |
| `call` | 2 | direct call to an external/imported function | **by design** — see below |
| `unmodeled-op` | **2** | "op not classified by the cycle model" | **synth** — largely FIXED, see below |

### RE-MEASURED on synth 0.57.0 — and the blocker INVERTED

The first cut of this measurement (synth 0.52.0) read **1 bounded of 31**, with
`unmodeled-op` at **9** — larger than `loop` at 8 — and recorded that as the
finding the release plan had not anticipated. synth#921 asked for the offending
opcode to be named; it turned out to be just `I64Const` + `I64Str` (synth#936).

**Both are now closed and the numbers moved:**

| | synth 0.52.0 | **synth 0.57.0** |
|---|---|---|
| BOUNDED | 1 of 31 | **3 of 31** |
| `unmodeled-op` | **9** | **2** |
| `loop` | 8 | **13** |
| `callee-unbounded` | 11 | 11 |
| `call` | 2 | 2 |

`unmodeled-op` fell 9 → 2, which is synth#936 landing. `loop` ROSE 8 → 13,
because functions that previously declined on an unmodelled opcode now get far
enough to decline on a loop instead — the gap moved, it did not vanish.

So the earlier finding is superseded: **`loop` is no longer merely larger than
`unmodeled-op`, it is 13 of 28 declines with 11 cascades behind it.** scry
loop-bound inference is now unambiguously the critical path for T4, and synth's
half is essentially done. Closing loop-bound inference would take coverage from
3/31 to at most 16/31 directly, plus whatever the 11 cascades release.

### `call` (2) is correct, not a gap

`gust:os/time@0.1.0#now` and `poll_task` are exactly the two functions that reach the
**seam** — `now` calls the imported `read32`, `poll_task` calls the embedder's
dispatch. An intra-procedural bound *should* stop at an import whose cost belongs to
the host. These two are the seam showing up in the WCET data, which is a good sign the
seam is real.

### `callee-unbounded` (11) is a cascade, not 11 problems

`exec#admit`, `exec#poll-round`, `sched/tasks#admit`, `sched/tasks#poll-round`,
`sched/tasks#set-deadline`, `cabi_realloc` and five more decline only because
something they call declines. Fix the leaves and these follow.

**So the real work is 17 leaves — 9 `unmodeled-op` + 8 `loop` — and 11 more come free.**

## The finding the plan did not anticipate — and the plan turned out right

`docs/releases/v0.7.0-plan.md` names **one** T4 blocker: *"scry loop-bound inference —
closes the `reason=loop` gap in the WCET sidecars, which is the one thing standing
between per-function bounds and a schedulability argument."*

On synth 0.52.0 that looked wrong: `unmodeled-op` (9) was **larger** than `loop` (8),
and `emit-wcet.sh`'s own header documented only `reason=call` and `reason=loop`, so
the third category was not in the track's written model of itself. That was the
finding, and it was accurate as measured.

**It has since been closed rather than confirmed.** synth#921 asked for the offending
opcode to be named; it was `I64Const` + `I64Str` (synth#936), both now fixed, and
`unmodeled-op` fell 9 → 2. What is left is `loop` at 13 — which is exactly the single
blocker the plan named in the first place.

So the honest record is: the plan under-specified the blocker set for one release
cycle, the gap was measured and routed upstream, synth closed it, and the plan's
original claim now holds. **scry loop-bound inference is the one thing standing
between per-function bounds and a schedulability argument** — with 11 cascade
declines behind it, closing it is worth far more than the 13 it names directly.

## Friction to route upstream (synth)

The `unmodeled-op` entries do not say **which** op:

    {
      "status": "declined",
      "name": "gust:os/time@0.1.0#elapsed",
      "reason": "unmodeled-op",
      "note": "op not classified by the cycle model"
    }

That is not actionable. With the opcode named, 9 declines become a specific, bounded
request against synth's cycle model — possibly a handful of instructions covering all
9. Without it, the only path is bisecting by hand. **Requested: include the offending
opcode (and ideally its offset) in the `unmodeled-op` decline.**

**Filed as synth#921** (https://github.com/pulseengine/synth/issues/921) — a follow-up
to the closed synth#778 that shipped `--emit-wcet`. The report carries the full decline
payload, the nine function names, and the tally; it also offers to run an instrumented
build against this composite, since it reproduces in three commands from a committed
input.

## What this does NOT say

- **Not a WCET for the OS.** One function has a bound. There is no partition budget
  here and nothing that could size one.
- **Not a DWT comparison.** No hardware was involved. Per the track's own rule, DWT
  high-water marks may only *falsify* a model, never size a budget.
- **Not a regression.** T4 was never run against the composite before — E2 is what
  made it possible. 1/31 is a baseline, not a decline from something better.

## Reproduce

    meld fuse benches/gust/drivers/gustos-components/fused-gustos.component.wasm \
      --memory shared -o /tmp/os.fused.wasm
    loom optimize /tmp/os.fused.wasm --passes inline --attestation false -o /tmp/os.loom.wasm
    synth compile /tmp/os.loom.wasm --target cortex-m3 --all-exports --relocatable \
      --emit-wcet -o /tmp/os.o          # writes /tmp/os.o.wcet.json
