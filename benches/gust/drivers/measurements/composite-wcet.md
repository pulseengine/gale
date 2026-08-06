# T4 on the whole OS — 1 bound of 31, and the reasons are not the ones we planned for

**Date:** 2026-08-06 · **REQ-OS-WCET-001** · synth 0.52.0, schema `synth-wcet-v1`,
core class `cortex-m3` · input: the E2 dissolved composite (31 functions)

E2 produced the first native object for the whole OS, which made T4 possible for the
first time. Running `synth --emit-wcet` over it gives the honest coverage number:

    BOUNDED    1  of 31
    DECLINED  30

The single bound is `gust:os/time@0.1.0#deadline` at **36 cycles**.

This is not a failure of the track — declines are loud and machine-readable by design,
which is the whole point of the sidecar. It is a measurement of how far T4 actually is
from feeding T3, and the answer is: further than the plan assumed.

## The 30 declines

| reason | n | what it means | whose problem |
|---|---|---|---|
| `callee-unbounded` | 11 | a directly-called callee is itself unbounded | **cascade** — resolves when the leaves do |
| `unmodeled-op` | 9 | "op not classified by the cycle model" | **synth** — the cycle model has holes |
| `loop` | 8 | backward branch without a statically-proven trip count | **scry** — loop-bound inference |
| `call` | 2 | direct call to an external/imported function | **by design** — see below |

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

## The finding the plan did not anticipate

`docs/releases/v0.7.0-plan.md` names **one** T4 blocker: *"scry loop-bound inference —
closes the `reason=loop` gap in the WCET sidecars, which is the one thing standing
between per-function bounds and a schedulability argument."*

That is no longer accurate. `unmodeled-op` (9) is **larger** than `loop` (8), and
`emit-wcet.sh`'s own header documents only `reason=call` and `reason=loop` — this third
category is not in the track's written model of itself. Closing loop-bound inference
alone would take coverage from 1/31 to at most 9/31, not to a schedulability argument.

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
opcode (and ideally its offset) in the `unmodeled-op` decline.** Filed once GitHub
Actions/API is back; recorded here so it is not lost.

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
