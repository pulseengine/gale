# DMA as Component-Model `own<buffer>` handoff over meld-fused memory

**Issue:** gale#124. **Status:** v1 core landed (verified FSM + WIT seam + rivet
trace); worked-example-on-silicon + synth region-marking are tracked follow-ons.

## Problem

DMA is the one path that violates wasm's linear-memory assumption: an external
agent (the DMA engine) mutates memory the engine otherwise treats as private.
Left unmodeled, this either (a) forces the buffer out of the verified world into
trusted native code, or (b) silently breaks synth's optimization assumptions
(synth would cache loads across a region the DMA engine is rewriting). We want
neither: keep DMA inside the verified-wasm world and shrink the trusted-native
surface to the irreducible atoms.

## Approach

Model a transfer as a Component-Model **ownership round-trip**. The DMA buffer is
a `resource`; `own<dma-buffer>` moves from wasm to the DMA agent on transfer and
returns on completion. Because meld fuses component linear memories into a single
merged-memory core, the handle carries only the **permission**, not the bytes —
zero-copy DMA with statically-checked exclusive ownership. The completion IRQ
returns the handle, so a transfer is naturally a `future<own<dma-buffer>>` driven
by the kiln scheduler.

```
wasm owns own<dma-buffer>
   │  transfer  (clean+DSB, program descriptor)      ← Wasm → Dma
   ▼
DMA owns  ─────────── completion IRQ ───────────┐
                                                │  return (invalidate+DMB, wake) ← Dma → Wasm
                                                ▼
                                    wasm owns own<dma-buffer> again
```

## Design decisions (resolved with the maintainer, 2026-07-01)

1. **Enforcement: BOTH composition-layer `own<T>` AND a Kani-proven runtime FSM.**
   CM `own<T>` move-checks live at the wac/meld composition layer, but synth
   dissolves *core* wasm — so the guarantee must also be present in the dissolved
   native artifact. We encode it twice, deliberately (ASIL-D bar): the WIT
   `resource`/`own<dma-buffer>` gives compile-time move semantics at compose time;
   a total, Kani-proven ownership state machine (`drivers/dma-own`) gives the
   runtime guarantee that survives dissolution. `DD-DMA-ENFORCE-001`.

2. **Region marking: a dedicated shared segment in the meld-fused memory.** synth
   identifies the DMA-owned window as externally-mutable via a concrete, marked
   segment in the merged memory (not a per-site attribute), which the verifier can
   also see. While wasm-owned the region is provably private (synth optimizes
   freely); during the DMA-owned window it is volatile (synth must not cache loads
   across the handoff). This is the synth#543 codegen interface (related: synth#390/loom#226).
   `DD-DMA-REGION-001`.

3. **v1 scope includes streaming/circular.** Not just the single round-trip: the
   ring/double-buffered case is modeled as per-chunk `own` handoff
   (`stream<own<chunk>>`), each chunk owned by exactly one side at a time.
   `REQ-DMA-STREAM-001`.

## The verified core (`benches/gust/drivers/dma-own`)

A total ownership state machine, Kani-proven, dissolved to native like every gust
primitive. `Owner ∈ {Wasm, Dma}` *is* the state (no "in flight" limbo → "no
ownerless state" is trivially true). Transitions return the new state **with** the
barrier op, so the caller cannot advance ownership without receiving (and the
bridge emitting) the paired coherency op — barrier-pairing **by construction**.

- **Single round-trip:** `start` (Wasm→Dma, clean+DSB), `complete` (Dma→Wasm,
  invalidate+DMB), `abort` (any→Wasm, invalidate+DMB — never ownerless).
- **Streaming ring:** `Ring<RING>` of per-chunk owners; `arm`/`reap`/`abort_all`.
- **Dissolve ABI:** `dma_start` / `dma_poll_complete` / `dma_abort` — scalar
  `u32`-encoded owner in/out, 0 linmem data (no r11 trampoline).

**Proven (6 Kani harnesses, 0 failures):** access-iff-wasm-owned; barrier pairing
on every path; abort never ownerless; round-trip integrity (double-start /
unpaired-complete are faults); streaming per-chunk exclusivity; ring reap + abort.

**Measured:** 218 B `.text` dissolved (synth 0.17 cortex-m3), 0 SRAM, TCB = 3
import relocations (`dma_program`, `dma_barrier`, `dma_irq_poll`).

## The seam (`gust:hal` `dma` interface)

The first `resource`/handle type in gust:hal: a `dma-buffer` resource, `read`/
`write` consuming `own<dma-buffer>` → `future<dma-buffer>`, `read-stream` →
`stream<dma-buffer>` (circular), `abort` taking `borrow<dma-buffer>`. The dissolved
FSM is its runtime twin; the TCB bridge implements the three atoms.

## Trusted-boundary semantics (TCB statement)

The counterparty is silicon, not a component. The CM type system + the Kani-proven
FSM protect the buffer **from our code** (wasm cannot touch it mid-transfer). The
shim + hardware protect the rest: the handle does **not** constrain the DMA engine
to stay within the region — that comes from the trusted shim programming the
descriptor correctly + silicon trust. Keep the descriptor-programming shim minimal
and audited. `REQ-DMA-TCB-001`.

## Async integration

The completion IRQ wakes a kiln waitable that resolves the `future<own<dma-buffer>>`
(`dma_poll_complete` wraps the split-phase `dma_irq_poll`: not-fired → yield;
fired → complete + re-own). DMA stays inside the fuel-bounded cooperative poll
model; no preemption; no host `Future` crosses the wasm boundary. Reuses the
existing IRQ→waker shim — no new trusted surface.

## Coherency / barriers

The ownership-transfer points are the coherency events. The transfer primitive
performs clean-before-DMA-write / invalidate-after-DMA-read + the DSB/DMB. On
Cortex-M3 these are no-ops (no cache), but the abstraction holds so it is correct
on M7/A-class. Making the barrier part of the handoff primitive makes it
structurally impossible to forget. `REQ-DMA-BARRIER-001` / proof `p2`.

## Open questions carried forward

- **Worked example on silicon** (SPI/ADC DMA round-trip, byte-identical non-DMA
  portions) — `VER-DMA-WORKED`, gated on a Renode DMA-controller model or a board
  with a probe. Not yet claimed.
- **synth honoring the shared segment as externally-mutable** — `FIND-DMA-SYNTH-001`,
  filed as synth#543 (related synth#390 / loom#226). Until it lands, correctness requires the DMA
  region not be optimized as private; the dissolved FSM itself is unaffected (it
  holds no buffer bytes).
- **Multi-master / bus contention** — out of scope for v1 (single-master assumed).
- **Barrier correctness on M7/A-class** — the abstraction is present and proven
  paired; validating the *actual* cache ops needs cache-bearing silicon (future).

## Acceptance criteria (issue #124) — status

- [x] CM resource type for a DMA buffer with own/borrow handles — `gust:hal` `dma`.
- [x] Transfer primitive consuming `own<buffer>`, descriptor + barrier at handoff,
      verified-by-construction pairing — `dma-own` + proof `p2`.
- [x] `future<own<buffer>>` completion wired to a kiln waitable via the IRQ shim —
      `dma_poll_complete` / `REQ-DMA-ASYNC-001`.
- [~] synth treats the DMA-owned window as externally-mutable; documented region
      marking — decision landed (`DD-DMA-REGION-001`); synth-side work filed
      (synth#390 / loom#226), not yet in synth.
- [x] TCB note enumerating exactly what remains trusted — `REQ-DMA-TCB-001` /
      RESULTS.md (3 atoms).
- [ ] Worked example on a real board — follow-on (`VER-DMA-WORKED`).
