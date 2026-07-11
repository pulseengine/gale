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
| ESP32-C3 (synth 0.12.0, flag-off) | 0.259 tick/call | 0.549 tick/call | 2.12× | identical |
| **ESP32-C3 (synth 0.40.0, RV32 levers default-on)** | **0.271 tick/call** | **0.500 tick/call** | **1.839×** | **identical** |

> **Re-anchored on real silicon (2026-07-11).** The committed
> `gust_mix-esp32c3.o` is now the **synth 0.40.0** dissolve (byte-reproducible:
> `synth compile <stripped gust_mix>.wasm -b riscv --target esp32c3 --all-exports
> --relocatable`, 476 B ELF, 0 relocations, md5 `d3526178…`). Flashed to the real
> ESP32-C3 (rev v0.4) and measured on the 16 MHz systimer over 200k iterations:
> native **271** vs dissolved **500** milliticks/call → **1.839×**, correctness
> **IDENTICAL** over the full input domain [0,2047] (mismatch=0). This confirms
> on-hardware what the byte deltas below predicted: the 0.12-era object was 492 B,
> the 0.40 object is **476 B (−16 B)** — exactly the cmp→select (−8 B) +
> shift-fold (−8 B) default-on levers — and that −16 B moves the on-silicon ratio
> **2.12× → 1.839×**. The RV32 flip-wave (synth#472) is now measured on silicon,
> not just predicted.

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
> **RESOLVED (synth 0.40.0, 2026-07-11): the on-hardware re-run is done.** The
> "whether it moves the on-hardware ratio needs a re-run on the board, which is
> pending" caveat is now closed — see the 1.839× row above. The committed object is
> re-pinned to the 0.40 dissolve (byte-reproducible), so the reproduce steps and the
> reported silicon number now agree. Four RV32 levers total (synth#484); two are
> default-on and both fire on gust_mix (−16 B, measured on silicon); the other two
> (i32 local-promote, const-addr-fold) remain flag-off (byte-identical on gust_mix).

## Same wasm, three architectures, all measured on silicon/sim

| arch | board | native vs dissolved | source |
|---|---|---|---|
| Cortex-M3 | STM32F100 (8 KB) | **1.73× (DWT, real)** | silicon/RESULTS-f100.md |
| Cortex-M4 | NUCLEO-G474RE | **1.448× (DWT, real, synth 0.40)** | silicon/RESULTS-g474re.md |
| **RISC-V RV32IMC** | **ESP32-C3** | **1.839× (systimer, real, synth 0.40)** | this file |

## Reproduce

```sh
brew install espflash            # (espflash 3.x — 4.x needs an app descriptor esp-hal 0.23 predates)
cd benches/gust/esp32c3
espflash flash --port /dev/cu.usbmodem<N> target/riscv32imc-unknown-none-elf/release/gust-esp32c3
cat /dev/cu.usbmodem<N>          # the app re-prints the ratio in a loop
```

Regenerate the dissolved object:
`synth compile <stripped gust_mix>.wasm -b riscv --target esp32c3 --all-exports --relocatable -o gust_mix-esp32c3.o`
