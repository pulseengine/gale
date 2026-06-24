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
