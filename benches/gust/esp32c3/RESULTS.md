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

> **synth 0.15.0 note (ARM-only levers):** the four perf levers that took the
> Cortex-M `gust_mix` from 2.63× → 1.81× (cmp→select fusion, stack-reload
> elimination, local promotion, immediate-shift folding) live in synth's **ARM
> backend**. The RISC-V backend has none of them — the dissolved esp32c3
> `gust_mix` is **byte-identical** under 0.12.0 and 0.15.0, so the 2.12× ratio is
> unchanged. Porting the levers to RV32 is tracked in **synth#472**; until then
> RISC-V is the lagging architecture.

## Same wasm, three architectures, all measured on silicon/sim

| arch | board | native vs dissolved | source |
|---|---|---|---|
| Cortex-M3 | STM32F100 (8 KB) | pending board | silicon/RESULTS-g474re.md |
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
