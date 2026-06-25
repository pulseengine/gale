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

| lowering | fn-only ticks/call | `.text` (gust_mix) | stack frame | toolchain |
|---|---|---|---|---|
| native (LLVM) | **0.40** | — | none | rustc/LLVM |
| dissolved, initial | 1.125 | — | 24 B | synth 0.11, no loom inline |
| dissolved, loom-inlined | 1.05 | 132 B | 8 B | loom 1.1.16 + synth 0.12.0 |
| dissolved, **4 levers** | **0.725** | **90 B** | 8 B | **loom 1.1.16 + synth 0.15.0** |
| **ratio vs LLVM** | **2.81× → 2.63× → 1.81×** | −32 % | — | — |

**Progress (measured, 2026-06-25): the ranked synth#428 asks shipped and
delivered.** synth landed all four ARM perf levers default-on across three
same-day releases — v0.13.0 cmp→select → IT-block predication fusion (the #1
ask), v0.14.0 redundant stack-reload elimination + i32 local promotion, v0.15.0
immediate-shift folding — each mapping 1:1 to a residual issue this file had
pinned on 0.12.0 (the materialized-boolean clamp, stack spill/reloads, the
6-register leaf prologue, `movw #8; lsl` instead of `lsl #8`). Re-measured on
`gust_codegen_bench`: dissolved `gust_mix` **1.05 → 0.725 ticks/call (−31 %)**
and **132 → 90 B (−32 %)**, taking the gap to native LLVM from **2.63× → 1.81×**,
correctness **bit-identical over [0,2047]**. loom v1.1.16's inline + whole-function
DCE (loom#228) had already merged the export wrapper (frame 24 → 8 B; 2.81× →
2.63×); synth's lowering closed most of the rest.
**Still open:** (1) the RISC-V backend has none of these levers — esp32c3
`gust_mix` is byte-identical 0.12.0↔0.15.0 (synth#472 tracks the port);
(2) the dense `control_step` still register-exhausts under default-on local
promotion (synth#474, confirmed on 0.15.0), so it builds with
`SYNTH_NO_LOCAL_PROMOTE=1` and gets only three of the four levers (580 → 568 B).
Full write-up + the ranked asks: `optimization/RECO-synth-cycles.md`;
cross-layer attribution: `optimization/ANALYSIS-where-to-optimize.md`.

Functional equivalence: `gust_mix` verified identical in wasmtime (the browser/host engine)
and in the synth-dissolved object — `1024→1500` centre, `0/512→1000`, `1536/2047→2000`.

### Honest verdict
- **vs native (LLVM):** synth's per-function *cycle* cost on the hot path is now
  **1.81×** (was 2.63×) after the four levers — closing on the project's
  10–20%-overhead thesis, with a clear remaining tunnel (RISC-V lever port
  synth#472; the local-promotion register-allocator fix synth#474). The
  larger-`.text` ratio narrowed in step (−32 %). **This is the gap still being
  driven down, and it is moving.**
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
