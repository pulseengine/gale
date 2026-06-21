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
