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

> **synth#472 UPDATE — the RV32 flip-wave began (synth 0.28).** The perf levers
> that took the Cortex-M `gust_mix` from 2.63× → 1.81× were ported to synth's
> **RISC-V backend** (issue closed 2026-07-02), first flag-off, and are now flipping
> default-on one at a time. As of **synth 0.28.0, `SYNTH_RV_CMP_SELECT` is
> DEFAULT-ON** (the #472 flip-wave, synth#601); i32 local-promotion's flip is HELD on
> a no-grow blocker. The RV32 port is **4 levers** (synth#484 scoping):
>
> | lever | env flag | default? (0.28) | fires on gust_mix? |
> |---|---|---|---|
> | cmp→select fusion (RV32 branch-comparator, synth#568) | `SYNTH_RV_CMP_SELECT` | **ON** | **yes, −8 B** |
> | immediate-shift-fold (`slli/srli/srai`, synth#487) | `SYNTH_RV_SHIFT_FOLD` | **ON (0.30.0, #611)** | **yes, −8 B** |
> | i32 local-promotion (s-register homing, synth#560) | `SYNTH_RV_LOCAL_PROMO` | off (flip HELD) | no (byte-identical) |
> | const-address-fold (RISC-V-specific, synth#491) | `SYNTH_RV_ADDR_FOLD` | off | no (byte-identical) |
>
> **Measured (synth 0.28.0 default vs 0.26.0, `gust_kernel.wasm -b riscv --target
> esp32c3 --all-exports --relocatable`):** with cmp→select now default-on, the
> esp32c3 dissolved kernel `.text` drops **144 → 136 B (−8 B)** by DEFAULT — no flag.
>
> **UPDATE (synth 0.30.0, #611): `SYNTH_RV_SHIFT_FOLD` also flipped default-on.**
> Re-measured on 0.30.1 (`gust_kernel.wasm -b riscv --target esp32c3 --relocatable`),
> shift-fold default vs `SYNTH_RV_SHIFT_FOLD=0`: kernel object **512 → 520 B**, i.e.
> **−8 B by DEFAULT** — exactly the residual the table above predicted. Two of the
> four RV32 levers are now default-on (cmp→select + shift-fold = −16 B vs the
> pre-flip baseline); the remaining two (i32 local-promote, const-addr-fold) stay
> flag-off (byte-identical on gust_mix). So the RISC-V lane is now improving in the
> shipped default, not just under flags.
>
> **Caveat — the 2.12× row above is a *silicon cycle* number, not codegen size, and
> it predates all of this** (measured flag-off on synth 0.12 on the real ESP32-C3
> systimer). The −8 B is a byte delta on the current toolchain; whether it moves the
> on-hardware ratio needs a re-run on the board, which is pending. The committed
> `gust_mix-esp32c3.o` (a 0.12-era single-function reference object) is left as-is —
> re-pinning it to 0.28 changes its shape beyond the lever delta (version drift) and
> the shipped figure that matters is the silicon cycle count, not the reference .o.

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
