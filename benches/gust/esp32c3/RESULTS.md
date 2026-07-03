# On-silicon results — ESP32-C3 (RISC-V RV32IMC), real hardware

The THIRD architecture: same wasm `gust_mix`, dissolved via `synth -b riscv
--target esp32c3`, run on a real ESP32-C3 (rev 0.4) through an esp-hal TCB.
Native (LLVM riscv32imc) vs dissolved (synth riscv), correctness-gated
bit-identical over [0,2047]. Timed on the 16 MHz systimer (the ESP32-C3 RISC-V
core does not implement the standard `mcycle` CSR — reading it traps), so the
absolute number is systimer-ticks, not cycles; the native-vs-dissolved **ratio**
is the codegen-quality figure and is on a common time base.

| | native | dissolved | ratio vs LLVM | correctness |
|---|---|---|---|---|
| ESP32-C3 (synth 0.12.0, flag-off) | 0.259 tick/call | 0.549 tick/call | **2.12×** | identical |

> **synth#472 UPDATE (closed 2026-07-02): the RV32 levers are now ported —
> flag-off by default.** The perf levers that took the Cortex-M `gust_mix` from
> 2.63× → 1.81× have been ported to synth's **RISC-V backend** and are all gated
> **off by default**, so the committed `gust_mix-esp32c3.o` and the flag-off
> **2.12×** silicon number above are **unchanged** — the levers do not fire
> unless their env var is set. The RV32 port is **4 levers** (synth#484 scoping):
>
> | lever | env flag | fires on gust_mix? |
> |---|---|---|
> | cmp→select fusion (RV32 branch-comparator, synth#568) | `SYNTH_RV_CMP_SELECT` | **yes, −8 B** |
> | immediate-shift-fold (`slli/srli/srai`, synth#487) | `SYNTH_RV_SHIFT_FOLD` | **yes, −8 B** |
> | i32 local-promotion (s-register homing, synth#560) | `SYNTH_RV_LOCAL_PROMO` | no (byte-identical) |
> | const-address-fold (RISC-V-specific, synth#491) | `SYNTH_RV_ADDR_FOLD` | no (byte-identical) |
>
> **Measured this run** (synth 0.26.0, `gust_kernel.wasm -b riscv --target
> esp32c3 --all-exports --relocatable`; the gust_mix compute kernel is `func_1`,
> the arithmetic callee): flag-on with all four levers shrinks the kernel
> **132 → 116 B (−16 B, −12.1%)** and the object's `.text` **144 → 128 B**. The
> shrink is entirely cmp→select + immediate-shift-fold (−8 B each, additive);
> local-promotion and const-address-fold leave gust_mix byte-identical (no
> promotable non-param i32 local, no foldable constant-address access in this
> kernel). This is a **codegen-size** delta on the current toolchain — the levers
> are **not adopted** here (no default-on flip, no silicon re-run), so the 2.12×
> ratio on real hardware still stands as the shipped figure.

## Same wasm, three architectures, all measured on silicon/sim

| arch | board | native vs dissolved | source |
|---|---|---|---|
| Cortex-M3 | STM32F100 (8 KB) | **1.73× (DWT, real)** | silicon/RESULTS-f100.md |
| Cortex-M4 | NUCLEO-G474RE | 2.21× (DWT, real) | silicon/RESULTS-g474re.md |
| **RISC-V RV32IMC** | **ESP32-C3** | **2.12× (systimer, real)** | this file |

## Reproduce

```sh
brew install espflash            # (espflash 3.x — 4.x needs an app descriptor esp-hal 0.23 predates)
cd benches/gust/esp32c3
espflash flash --port /dev/cu.usbmodem<N> target/riscv32imc-unknown-none-elf/release/gust-esp32c3
cat /dev/cu.usbmodem<N>          # the app re-prints the ratio in a loop
```

Regenerate the dissolved object:
`synth compile <stripped gust_mix>.wasm -b riscv --target esp32c3 --all-exports --relocatable -o gust_mix-esp32c3.o`
