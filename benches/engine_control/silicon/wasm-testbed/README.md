# wasm-cross-LTO testbed

**Quick regression check:** `./run_testbed.sh` — rebuilds all 3 algorithm functions from source and verifies both synth-compiles (catches codegen regressions, e.g. v0.11.19 register-exhaustion) and wasmtime functional correctness vs verified ground-truth vectors. Exit 0 = ALL GREEN. Run on every new synth/loom release.


A three-layer testbed for validating and benchmarking the
`clang → wasm-ld → loom → synth → ARM` pipeline that dissolves the C↔Rust /
caller↔callee seam for Cortex-M kernel/algorithm code. Built while driving the
on-silicon `wasm-cross-LTO` work (see `../boards/nucleo_g474re/NOTES-wasm-cross-lto-spike.md`).

Each layer answers a different question, cheap → expensive:

| Layer | Tool | Question | Needs HW? |
|-------|------|----------|-----------|
| 1. Functional (wasm) | `wasm_oracle.sh` (wasmtime) | Does the wasm compute the right values? | no |
| 2. Functional (ARM) | `arm_harness.py` (unicorn) | Does **synth's Cortex-M output** compute the right values? | no |
| 3. Performance | `silicon-microbench/` (Zephyr + DWT) | How many **cycles** on real silicon, vs native gcc? | NUCLEO-G474RE |

The point: layers 1–2 catch functional bugs (and isolate whether a fault is in
our wasm vs synth's lowering) **without** a board; layer 3 is reserved for the
actual cycle measurement once functionality is proven.

## Pipeline (per function)

```sh
clang --target=wasm32-unknown-unknown -O2 -nostdlib -c fn.c -o fn.o
wasm-ld --no-entry --export=<fn> --allow-undefined --gc-sections fn.o [tables.o] -o fn.wasm
loom optimize fn.wasm --passes inline --attestation false -o fn.loom.wasm     # dissolves caller↔callee seams (loom ≥ v1.1.6 for memory-reading callees)
synth compile fn.loom.wasm --target cortex-m4f --all-exports --relocatable -o fn.o   # ARM ET_REL
```

## Layer 1 — wasm functional oracle (`wasm_oracle.sh`)

Runs each exported function in wasmtime and diffs against a native reference.
Covers `control_step_decide` (engine), `controller_step_decide` + `filter_axis`
(flight). **Caveat:** `wasmtime --invoke` must be one-shot per process — capturing
it inside a shell loop returns empty; call each vector explicitly.

## Layer 2 — ARM functional harness (`arm_harness.py`)

Executes synth's Thumb-2 output under unicorn. Validated against a trivial
`add3(a,b,c)=60`, so a divergence is a real synth-lowering bug, not a harness
artifact. Memory model: code @ 0x08000000, RAM @ 0x20000000; for table/pointer
functions, `fp(r11)` = linear-memory base (the **fp=0 trampoline** makes
`[fp+ptr]` a native deref). This layer is how we pinned synth#210 (a live
pointer/param register is clobbered mid-body during memory access — generalizes
from the engine tables to pointer-struct reads like `filter_step`).

## Layer 3 — silicon micro-bench (`silicon-microbench/`)

A standalone Zephyr app that links a synth ET_REL object and times the function
with DWT CYCCNT @170 MHz, head-to-head vs the native gcc build of identical C
(both called via a volatile fn-pointer with the Thumb bit set — synth#170 emits
no `$t` symbol — so call overhead is identical and the delta is pure codegen).
Overhead floor = min over 200 back-to-back CYCCNT reads (= 1 cyc; do **not** use
a single sample). Build/flash:

```sh
export ZEPHYR_BASE=.../pulseengine/zephyr
west build -b nucleo_g474re -d /tmp/build silicon-microbench   # edit CMakeLists link target to your fn.o
# capture BEFORE reset: python3 ../capture.py --port <VCP> --sentinel "=== END ===" --out cap.txt &
openocd -f interface/stlink.cfg -f target/stm32g4x.cfg -c "program build/zephyr/zephyr.elf verify reset exit"
```

## Results to date (NUCLEO-G474RE, synth v0.11.20 / loom v1.1.10)

| function | synth (wasm-cross-LTO) | native | ratio | status |
|----------|------------------------|--------|-------|--------|
| `k_sem_give` handoff | 907 cyc | 471 (LLVM-LTO) | 1.92× | on silicon |
| `filter_axis` | 46 cyc | 19 | 2.42× | on silicon |
| `control_step` (engine algo) | 168 cyc | 81 | 2.07× | on silicon (full Opt 1: guard-elision + reciprocal-mul) |
| `flight_algo` (filter+controller, fully dissolved) | 315 cyc | 99 | 3.18× | on silicon (loom v1.1.10 full dissolution) |

All four functionally correct on hardware. **Synth correctness fixed** (#210 param-clobber,
#212/#215 R12-scratch — all closed). **Key perf finding:** Opt 1b (reciprocal-multiply) is
near-neutral on Cortex-M4 (−4 cyc on control_step) because M4 has a fast HW `udiv`; the ~2×
gap is **general codegen — register allocation + constant-CSE (Opt 3)**, confirmed by the
filter_axis decomposition (~17 cyc base-codegen vs ~10 cyc divide) and by control_step staying
2× with divides fully optimized. Remaining synth work (issue #209): **Opt 3** — constant-CSE
(control_step is ~28% constant materialization, ~1/3 redundant) + reclaim r9/r10 (both free).

## Adding a function

1. Write `fn.c` as scalar-in/scalar-out where possible (avoid host pointers; pack
   multiple outputs into a return). Pointer/table functions need the fp trampoline
   and are currently synth#210-blocked.
2. Run the pipeline above → add a vector set to `wasm_oracle.sh` and `arm_harness.py`.
3. Only once layers 1–2 pass, wire it into `silicon-microbench/` and measure.

## Multi-target layout (`arch/`) — shared front-half vs per-target back-half

The shared front-half lives at the testbed root (algorithm C sources, the wasm modules, loom dissolution,
the wasmtime oracle + verified vectors — all architecture-independent). The per-target **back-half** is a
small adapter under `arch/<target>/`:

- **`arch/riscv/`** — RISC-V adapter. `run_native.sh` builds the shared algorithm sources with
  `riscv64-elf-gcc -march=rv32imac_zicsr`, links synth's `riscv-runtime` (startup.c + linker.ld), and runs
  on `qemu-system-riscv32 virt -icount` with an `mcycle`-CSR harness (`main.c`). NATIVE baseline (qemu
  `-icount` = instruction-count proxy, **not** silicon cycles): filter_axis 17, control_step 62, flat_flight 75
  (all correct). For the synth path: `synth compile <fn>.loom.wasm -b riscv -t rv32imac` and link its `.o`
  in place of the native obj (RISC-V linear-memory base = `s11`, the analogue of ARM `r11`). Real RV silicon
  cycles need ESP32-C3 or Renode (neither currently wired here).
- **arch/arm** — the ARM adapter is the existing `../silicon-microbench/` (Cortex-M Zephyr + DWT CYCCNT on
  NUCLEO-G474RE) + the `arm_harness.py` unicorn layer.

So: write+verify the wasm/oracle **once**; retarget by swapping only the `arch/<target>/` adapter
(synth backend `-b arm`/`-b riscv`, the linmem-base trampoline register, the runtime/linker, and the cycle harness).
