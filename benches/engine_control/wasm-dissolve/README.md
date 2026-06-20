# engine-control, dissolved to wasm — a real-algorithm optimization surface

The `engine_control` bench's pure algorithm (`../src/control.c`: `control_step`)
run through gale's **maximal-wasm dissolve path** — the same `wasm → loom inline
→ synth → cortex-m` pipeline that ships the wasm-dist primitives and boots gust —
and compared head-to-head against the rustc/clang LLVM floor.

It exists for two reasons (gale#74, task #26):

1. **A meatier optimization surface for synth & loom.** The verified `*_decide`
   functions are tiny (a few branches). A real control algorithm exercises the
   passes the synth codegen proposal ([synth#390]) actually targets:
   - **2-D table lookups** `spark_advance_table[rb][lb]` → addressing-mode folding
   - **constant divides** `/500 /5 /80 /1000` → strength reduction
   - **saturating clamps** → branch-on-flags
2. **A measurable before/after.** Re-run `compare.sh` after synth lands the
   regalloc / addressing-mode work and the ratio moves — the proposal stops being
   a claim and becomes a tracked number.

## Measured now (synth 0.11.50 / loom 1.1.14, cortex-m4f)

| function | native (clang→thumbv7m) | dissolved (wasm→loom→synth) | ratio |
|---|---|---|---|
| `control_step` (the algorithm) | **180 B** | **378 B** | **2.1×** |
| whole dissolved `.text` | — | **582 B** | no runtime |

**Functional equivalence (3-runtime):** `host-LLVM` vs `wasmtime` over 8 input
vectors — **8/8 identical**. The dissolved `.o` is size-compared here; its
on-target cycles run on the MCU/Renode lane.

### Why 2.1× and not gust's 3.9×

gust's scheduler hot path is **spill-dominated** (loop-live pointer reloaded from
the same stack slot 10+ times — synth has no register allocator yet). This
algorithm is **arithmetic-dominated** and largely straight-line, so synth's
missing regalloc costs proportionally less. The two benches bracket the codegen
gap: 2.1× (arithmetic) … 3.9× (spill-heavy). Closing [synth#390]'s regalloc pass
should pull both toward the project's 10–20%-overhead thesis.

## Reproduce

```sh
# macOS: brew llvm provides llvm-size/llvm-nm
export PATH="/opt/homebrew/opt/llvm/bin:$PATH"
./compare.sh                 # default synth target cortex-m4f
SYNTH_TARGET=cortex-m3 ./compare.sh
```

Exits non-zero if the 3-runtime functional differential ever diverges — so this
doubles as a dissolve-pipeline regression check, not just a size report.

The same `control_step_packed` export (pure scalar ABI) is what the wider
3-runtime testbed (task #27) will drive in the browser and under wasmtime+kiln
alongside the verified `gale::` components.

[synth#390]: https://github.com/pulseengine/synth/issues/390
