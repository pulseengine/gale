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
