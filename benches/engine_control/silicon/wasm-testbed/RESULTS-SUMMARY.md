# wasm-cross-LTO — consolidated results

The `clang → wasm-ld → loom (dissolve seam) → synth → ELF` pipeline, measured against the
native/LLVM-LTO/rustc-direct alternatives. ARM = real NUCLEO-G474RE silicon, DWT CYCCNT @170 MHz,
min-over-200 (or bench median where noted). RISC-V = `qemu_riscv32 -icount` (instruction-count
**proxy**, not silicon cycles). Single source of truth; see NOTES-wasm-cross-lto-spike.md for provenance.

## Kernel primitive (the headline)

| primitive | wasm-cross-LTO | LLVM-LTO | native gale | notes |
|-----------|---------------|----------|-------------|-------|
| `k_sem_give` handoff (ARM silicon) | **907** cyc | 471 | — | 1.92×; dissolved drop-in, seam folded (no `bl ..._decide`), n=148 median |
| `k_mutex_unlock` (ARM silicon) | *pending #237* | — | **124** (ref) | dissolves + links (v0.11.28); native-drop-in gated on the `--native-pointer-abi` ABI fix |

## Algorithm functions (value-in/value-out — dissolve cleanly, both backends)

| function | ARM silicon (synth / native) | RISC-V icount v0.11.27 (synth / native) |
|----------|------------------------------|------------------------------------------|
| `filter_axis`     | 46 / 19 = **2.42×** | 23 / 17 = **1.35×** |
| `control_step` (engine algo) | 168 / 81 = **2.07×** | 129 / 62 = **2.08×** |
| `controller_step` | — | 100 / 49 = **2.04×** |
| `flat_flight` (flight algo, composed) | 315 / 99 = **3.18×** | 181 / 75 = **2.41×** |

All functionally correct on both backends (RV32 funccheck 10/10, ARM funccheck 6/6, wasmtime oracle).
flight_control bench wasm-LTO variant builds + runs the dissolved algorithm on G474RE (Phase 5).

## The two open optimization/expansion levers (maintainer-side)

1. **const-CSE + cross-statement local promotion (synth#209)** — the composed path's remaining gap.
   `flat_flight` is 57% redundant constant materializations (37 const-loads / 16 distinct) + 17 stack
   spills; the v0.11.27 caller-saved-preference fix nearly halved the leaves (filter 2.18→1.35×) but
   barely moved the composed path (2.57→2.41×). const-CSE is the next lever.
2. **native-call ABI / `--native-pointer-abi` (synth#237, v0.11.29 in progress)** — unblocks
   host-pointer primitive drop-ins (mutex, sem) by emitting wasm statics as base-independent
   `.data`/`MOVW-MOVT` relocations while host-pointer args stay `base=0`. Re-measure staged
   (`mutex-microbench/remeasure_wasm_lto.sh`) — one command when the flag lands.

## Headline
The PulseEngine pipeline dissolves the C↔Rust seam at wasm-IR level and produces correct silicon
output at ~2–3.2× native (widening with composition), with LLVM-LTO-parity codegen shape. The gap is
general codegen (regalloc/const-CSE = #209), confirmed cross-target on ARM **and** RISC-V — the
single retargetable lever. Host-pointer primitive drop-ins await the native-call ABI (#237).
