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
| `control_step` (engine algo) | 158 / 67 = **2.36×**§ | 129 / 62 = **2.08×** |
| `controller_step` (7-arg) | 169 / 61 = **2.77×**† | 100 / 49 = **2.04×** |
| `flat_flight` (flight algo, composed) | 255 / 103 = **2.48×**‡ (262→261 #250 AND, →255 #262 clamp) | 181 / 75 = **2.41×** |

All functionally correct on both backends (RV32 funccheck 10/10, ARM funccheck 6/6, wasmtime oracle).

‡ `flat_flight` ARM refreshed 2026-06-04 to **262/103 = 2.54×** (loom 1.1.10 + synth 0.11.30, reproducible `flat_flight-microbench/`, SELFCHECK 0x07fdf307). The prior **315/3.18×** was synth **v0.11.18** — stale; the caller-saved-preference fix (v0.11.27) and later improvements already cut it. 262 includes the fp-setup trampoline (~8 cyc); body ~254.

§ `control_step` ARM refreshed 2026-06-05 to **158/67 = 2.36×** (v0.11.34, mla un-wired; mla-on would be 156). loom 1.1.10 + synth 0.11.34, reproducible `control-step-microbench/`, SELFCHECK 2165333). Prior 168/81 was an older synth. Buffer-harness (tables copied into a RAM linmem buffer, r11=base).

† `controller_step` has 7 args; synth’s cortex-m convention passes args in **r0–r7** (not AAPCS r0–r3+stack), so a C/Zephyr caller needs an arg-shuffle trampoline (`controller-microbench/ctl_tramp.S`). The 169 includes that ~8-cyc marshalling (native called directly); the dissolved body alone is ~161. SELFCHECK 0x05e33e81 == native on G474RE.
flight_control bench wasm-LTO variant builds + runs the dissolved algorithm on G474RE (Phase 5).

## Bigger example — flight_control macro bench (Phase 5, composed)

The flight_control bench composes 5 Zephyr primitives (ring_buf, sem, mutex, msgq, condvar) on a
100 Hz loop; `GALE_FC_WASM_LTO=ON` dissolves the ISR-side flight algo (`filter_step`+`controller_step`)
via wasm-cross-LTO. On real G474RE silicon, full 5-step sweep, **no fault**, current toolchain
(loom 1.1.10 + synth 0.11.30):

| metric (in-bench `algo`) | wasm-cross-LTO | native | ratio |
|--------------------------|----------------|--------|-------|
| ISR filter precompute     | **157** cyc     | 141    | **1.11×** |

The in-context overhead is only **~11%** — far tighter than the isolated microbenches (flat_flight 2.54×,
controller 2.77×), because the dissolved algo is a fraction of per-sample work (handoff/lock/post/round
are common to both builds). The bigger example is a working testbed for functionality *and* optimization.

## The two open optimization/expansion levers (maintainer-side)

1. **const-CSE + cross-statement local promotion (synth#209)** — the composed path's remaining gap.
   `flat_flight` (262 cyc ARM, current) is 61% redundant constant materializations (34 const-loads / 13 distinct; clamp
   bounds `#0x7e`/`#0x7f` ×6 each) + 17 stack spills (refreshed on loom 1.1.10 + synth 0.11.29); the v0.11.27 caller-saved-preference fix nearly halved the leaves (filter 2.18→1.35×) but
   barely moved the composed path (2.57→2.41×). const-CSE is the next lever.
2. **native-call ABI / `--native-pointer-abi` (synth#237, v0.11.29 in progress)** — unblocks
   host-pointer primitive drop-ins (mutex, sem) by emitting wasm statics as base-independent
   `.data`/`MOVW-MOVT` relocations while host-pointer args stay `base=0`. Re-measure staged
   (`mutex-microbench/remeasure_wasm_lto.sh`) — one command when the flag lands.

## Headline
The PulseEngine pipeline dissolves the C↔Rust seam at wasm-IR level and produces correct silicon
output at ~2–2.6× native (widening with composition), with LLVM-LTO-parity codegen shape. The gap is
general codegen (regalloc/const-CSE = #209), confirmed cross-target on ARM **and** RISC-V — the
single retargetable lever. Host-pointer primitive drop-ins await the native-call ABI (#237).
