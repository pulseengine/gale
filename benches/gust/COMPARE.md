# gust comparative bench — native vs wasm-dissolved (and vs WAMR-class)

The point of gust is the **maximal-wasm** thesis: author the kernel as wasm, dissolve it
to native (meld → loom → synth), ship with no runtime. This bench measures the cost of
that choice against the two reference points — **native rustc/LLVM** (the floor) and a
**WAMR-class on-device runtime** (the incumbent) — so the claim is evidence, not rhetoric.

## Measured now (codegen, same source two toolchains, M-class Thumb-2)

Same `gust_poll` (kiln-async `poll_round` + failsafe mixer closure) and `gust_mix`,
compiled two ways: **native** = rustc/LLVM → thumbv7m; **dissolved** = the identical Rust
→ wasm32 → loom inline → synth → cortex-m3 relocatable.

| function | native (rustc→thumbv7m) | dissolved (wasm→synth→cm3) | ratio | how to read it |
|---|---|---|---|---|
| `gust_poll` (scheduler hot path) | **208 B** | **816 B** | **3.9×** | synth codegen vs LLVM, today |
| `gust_mix` (Q8 failsafe mixer) | 12 B | 44 B | 3.7× | — |
| whole dissolved kernel `.o` `.text` | — | **1402 B** | — | the *entire* kernel, **no runtime** |

### Cycle cost (not just size) — `gust_codegen_bench`

`cargo run --release --bin gust_codegen_bench` times the SAME `gust_mix` lowered
both ways under one SysTick harness (qemu `-icount`, deterministic; instr ≈
cycles on M3), with a correctness gate (native ≡ dissolved, bit-identical over
[0,2047]) before timing:

| lowering | fn-only ticks/call | callee-saves | stack frame | toolchain |
|---|---|---|---|---|
| native (LLVM) | **0.40** | `{r7,lr}` | none | rustc/LLVM |
| dissolved, initial | 1.125 | `{r4–r8,lr}` ×2 | 24 B | synth 0.11, no loom inline |
| dissolved, **current** | **1.05** | `{r4–r8,lr}` | 8 B | **loom 1.1.16 + synth 0.12.0** |
| **ratio vs LLVM** | **2.81× → 2.63×** | — | — | — |

**Progress (measured):** loom v1.1.16 landed the inline + whole-function DCE +
arg-forwarding (loom#228) — the export wrapper is now merged into the body (no
`bl`, no second prologue), shrinking the frame 24 B → 8 B and the gap 2.81× →
**2.63×**. The residual is now **entirely synth's arithmetic lowering**
(synth#428, still open): the merged `gust_mix` under synth 0.12.0 still emits a
6-register leaf prologue, stack spill/reloads of locals, a register shift
(`movw #8; lsl r,r,r` instead of `lsl #8`), and the compare→select clamp as a
materialized-boolean-then-test (`cmp;ite;mov#1/#0;cmp#0;it;movne`) — twice.
synth 0.12.0 shipped DWARF + the spill-pressure CI-gate (#441), not the lowering
fixes. Full write-up + the ranked asks: `optimization/RECO-synth-cycles.md`;
cross-layer attribution: `optimization/ANALYSIS-where-to-optimize.md`.

Functional equivalence: `gust_mix` verified identical in wasmtime (the browser/host engine)
and in the synth-dissolved object — `1024→1500` centre, `0/512→1000`, `1536/2047→2000`.

### Honest verdict
- **vs native (LLVM):** synth's per-function codegen is ~**3.9×** larger on the hot path —
  well above the project's 10–20%-overhead thesis. This is the gap to drive down: synth
  backend optimization + loom#219 full seam-inlining. **This number is the goal-post.**
- **vs WAMR-class runtime:** dissolve wins decisively on *total* footprint — the whole
  kernel is **1.4 KB with zero resident runtime**, versus a WAMR interpreter/AOT-loader of
  **~50–64 KB** plus per-module RAM (and 10–50× interpreter energy). WAMR structurally
  cannot reach gust's 8 KB / no-runtime class at all.
- Net: gust already wins the *footprint/energy/device-class* argument vs the incumbent; the
  open work is closing the *per-function codegen* gap vs native LLVM.

## Pending (needs synth#383 and/or hardware)

| metric | status | blocker |
|---|---|---|
| on-MCU cycles (dissolved `gust_poll`) | pending | Renode run + dissolved image boot (synth#383 `.bss` link) |
| whole dissolved-image flash/RAM | pending | synth#383 (8 KB `.bss` link) |
| battery / sleep-current (wohl) | pending | dissolved image + power model |
| head-to-head vs **WAMR-AOT** (size + cycles) | pending | build the same logic under WAMR-AOT |
| native thumbv7m cycles (baseline) | doable now (Renode) | — |

The native bench (`./run-bench.sh`, qemu `-icount`) already gives `poll_round` = 3.10 ticks
O(1); the dissolved-on-MCU cycle number slots in once synth#383 lands and the dissolved
image boots (task #20).

## Reproduce the codegen table
```sh
./compare-codegen.sh   # builds native thumbv7m + dissolved cortex-m3, prints the size table
```
