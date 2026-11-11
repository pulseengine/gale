# synth 0.54.0 on gale — the default path gains nothing; the flag is where the win is

**Date:** 2026-08-05 · **Baseline:** synth 0.52.0 (gale's current pin, #208)
**Verdict: do not bump for size.** 0.54.0's default codegen is size-identical to
0.52.0 on every gale input measured, and costs 4 B on one of them. The gain the
release advertises is behind an environment flag that is off by default.

## Method

Synth is the *only* variable. Each measurement compiles a **committed** wasm
input with different synth binaries and nothing else:

    synth compile <input> --target cortex-m3 --all-exports --relocatable -o out.o
    arm-none-eabi-size out.o        # .text

Full-chain rebuilds were deliberately **not** used for attribution. A
`build-breadth.sh` rebuild in this environment does not reproduce the committed
object even at the 0.52.0 pin — the local meld is 0.41.3 where the scripts
default to meld-0.41.0 — so a full-chain number would fold meld/loom drift into
a figure labelled "synth". That is the same conflation the cabi-realloc-extern
measurement had to correct for; the isolation is the point.

## Result 1 — the default path is flat, and 0.53.0 costs 4 B

`.text` bytes, cortex-m3:

| input | 0.49.0 | **0.52.0** | 0.53.0 | 0.54.0 | 0.52 → 0.54 |
|---|---|---|---|---|---|
| `os-node/repro-757/loom.wasm` | 1782 | **1770** | 1770 | 1770 | 0 |
| `wasm-kernel/fused.wasm` | 640 | **620** | 620 | 620 | 0 |
| `wasm-kernel/gust_kernel.wasm` | 1262 | **1194** | 1198 | 1198 | **+4** |
| `drivers/breadth` (full chain, synth-only delta) | — | 2442 | — | 2442 | 0 |

**The gains are already banked.** An earlier pass of this measurement used 0.49.0
as the baseline and reported −12 / −20 / −64 B. That was wrong about what is
*new*: 0.49 → 0.52 is where those bytes went, and gale re-pinned to 0.52.0 in
\#208. Against the version gale actually ships, 0.54.0 is flat.

**The +4 B is one function and it is the right trade.** It appears in 0.53.0 and
persists: `func_3` / `gust_poll` (two names, one body) grows `0x2b4 → 0x2b6` —
a single Thumb instruction, counted twice plus alignment. 0.53.0 shipped
*range-realloc cross-barrier live-in soundness — the pass AND its validator*
(synth#872, PR #888). A pass that stops taking a transform it could not justify
across a barrier is **supposed** to cost a couple of bytes. Recorded as a
soundness cost, not a regression to chase.

## Result 2 — `SYNTH_GRAPH_ALLOC=1` is worth real bytes

The graph-colouring allocator (synth#242, VCR-DEC-001) ships **flag-off**. Turned
on, on the same three committed inputs, same 0.54.0 binary:

| input | flag-off | flag-on | delta |
|---|---|---|---|
| `loom.wasm` | 1770 | 1750 | **−20 B (−1.1%)** |
| `fused.wasm` | 620 | 604 | **−16 B (−2.6%)** |
| `gust_kernel.wasm` | 1198 | 1186 | **−12 B (−1.0%)** |

Across the three inputs it took **15 whole-function colourings, each
`APPLIED (validated)`**, and declined 12 — 7 identity-colouring, 2
unreachable-block, 2 unmodeled-op, 1 single-block. It validates each rewrite
(`validate_cfg_rewrite`) and falls back to the shipping allocator when it
cannot, which is the disposition this project wants from an optimizer.

Defined and undefined symbol sets are **identical** flag-on vs flag-off on all
three inputs, so the objects still link the same way.

## What this does NOT establish

- **Not behaviour.** Size and symbol parity are not correctness. The flag-on
  objects have **not** been through the Renode device-class gate
  (`//:gust-renode`, `//:gust-control-renode`, `//:gust-f100-renode`), which is
  the oracle that asserts the spark/fuel line and the deterministic instruction
  count. Nothing here licenses shipping the flag on.
- **Not a WCET claim.** No cycle measurement was taken; `.text` is space.
- **Not silicon.** Everything above is host-side compilation of committed inputs.

## Recommendation

1. **Hold the pin at 0.52.0** for size reasons. There is no default-path win in
   0.53/0.54 for gale, and the one delta is a soundness cost.
2. If we bump for other reasons (the aarch64 surface completes in 0.54.0 and the
   oracle-wiring gate went 69-unwired → 145-wired), do it as its own PR and let
   the Renode gate decide — the committed ELFs are what that gate reads, so a
   bump only becomes gateable once objects are regenerated.
3. **Feed Result 2 upstream.** synth#242 is asking exactly this question — this
   is three real embedded inputs, 15 validated colourings, and a measured
   −1.0 to −2.6% with the accept/decline breakdown.

Reproduce:

    for v in 0.52.0 0.53.0 0.54.0; do
      "$HOME/pe-toolchain/synth-$v/synth" compile benches/gust/wasm-kernel/gust_kernel.wasm \
        --target cortex-m3 --all-exports --relocatable -o /tmp/k-$v.o
      arm-none-eabi-size /tmp/k-$v.o
    done
    SYNTH_GRAPH_ALLOC=1 SYNTH_GRAPH_ALLOC_STATS=1 "$HOME/pe-toolchain/synth-0.54.0/synth" \
      compile benches/gust/wasm-kernel/gust_kernel.wasm --target cortex-m3 \
      --all-exports --relocatable -o /tmp/k-on.o
