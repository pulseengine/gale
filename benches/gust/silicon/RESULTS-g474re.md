# On-silicon results — NUCLEO-G474RE (Cortex-M4), real DWT cycles

`silicon/run.sh g474re` — probe-rs (STLink V3) flash + semihosting capture,
`gust_mix` native (LLVM thumbv7em) vs dissolved (synth --target cortex-m4),
DWT CYCCNT, 20000 iters, baseline-subtracted, correctness-gated bit-identical
over [0,2047]. probe-rs 0.31.0.

| synth codegen | native | dissolved | ratio vs LLVM | correctness |
|---|---|---|---|---|
| flag-off (synth 0.12.0) | 29.0 cyc/call | 64.0 cyc/call | **2.21×** | identical |
| `SYNTH_CMP_SELECT_FUSE=1` | 29.0 cyc/call | **58.0 cyc/call** | **2.00×** | identical |

- The cmp→select fusion (synth#428 lever 1) is a measured **−6 cyc/call (−9%)** on
  real M4 silicon, correctness preserved → **synth#428 precondition-#2 (G474RE DWT
  no-regression) satisfied**.
- Real-M4 ratio (2.21× off) is lower than the qemu `-icount` ratio (2.63×) — qemu
  counts instructions; the M4 pipeline/timing differs.
- STM32F100 (Cortex-M3) results: pending the board (run `silicon/run.sh f100`).

## gust_control on real M4 — north-star rung 2 (the stack, on silicon)

`gust_control` (kiln-async scheduler driving the dissolved engine_control loop)
flashed on the physical NUCLEO-G474RE via probe-rs:
- `control_step(3000,50,80,0)` = spark 33° / fuel 2300µs — **matches C/wasmtime**
- 5000 control ticks on the kiln/gust stack; last `(4700,75,80,0)` = spark 38° / fuel 2440µs (== wasmtime)

So the whole north-star runs on real hardware: components (CM) → meld fuse / synth
dissolve → driven by the kiln-async scheduler on gust, Cortex-M4, no runtime.
control_step needs `--native-pointer-abi` + `--shadow-stack-size 8192` + the r11=0
TCB trampoline (it reads its tables off the linmem base the scheduler clobbers), and
the cortex-m4 `.o` (the cortex-m3 one won't link into a thumbv7em image). F100/M3
pending the board.


## Re-anchored on real M4 — the 2026-07 ladder (synth 0.40.0)

`silicon/run.sh g474re` — probe-rs 0.31.0, STLink V3, DWT CYCCNT, 20000 iters,
baseline-subtracted. Same harness as the 0.12.0 row above; only the dissolved
`gust_mix-cm4.o` changed (the 2026-07 perf work).

| synth codegen | native (LLVM) | dissolved | ratio vs LLVM | sound domain |
|---|---|---|---|---|
| 0.12.0 flag-off (historical) | 29.0 cyc | 64.0 cyc | 2.21× | full [0,2047] |
| **0.40.0 `SHIFT_MASK_ELIDE` (current pin)** | 29.0 cyc | **42.0 cyc** | **1.448×** | full [0,2047] ✓ |
| 0.40.0 `SYNTH_FACT_SPEC` (proof-carrying) | 29.0 cyc | 41.0 cyc | 1.413× | **[524,1524] only** |

- **The 2026-07 ladder is confirmed on real M4 silicon: 2.21× → 1.448×.** Native
  LLVM is unchanged (29.0 cyc/call — same compiler), so the −22 cyc/call (64→42) is
  entirely the synth-side codegen work (mask elision + the accumulated 0.16→0.40
  levers). Real-M4 **1.448×** tracks the qemu `-icount` **1.50×** closely (the M4
  pipeline hides a hair more of the dissolved overhead than instruction-counting).
- **The `SYNTH_FACT_SPEC` row is a proof-carrying, *specialized* measurement.** It is
  faster (1.413×) but **sound only under the carried invariant `ch ∈ [524,1524]`** —
  the bench's full-domain `[0,2047]` correctness gate **correctly FAILS** it, which is
  the precondition made visible on hardware, not a defect. A clean in-range silicon
  floor bench (mirroring `gust_floor_bench`'s proven-range harness, where the
  source-level floor is 0.45×) is the follow-on. Not a committed pin (verify-only
  synth build; premise not yet default).
- **Fixed** a latent `run.sh` arg-parse bug (`${1:?... {g474re|f100}}` appended a
  literal `}` to `$BOARD`). Two probes present (board ST-LINK + an ESP-JTAG); pass
  `--probe <STLink VID:PID:serial>` explicitly.
