# Cross-architecture: one wasm, two instruction sets (2026-07-29, synth 0.52.0)

The thin-seam drivers are written once, built once to wasm, and lowered to **both** ARM
Cortex-M and RISC-V RV32 from that same input. This is the "change the tires, not the
car" claim reduced to something checkable: the driver does not change, the native
residual under it does.

Reproduce: `bash benches/gust/drivers/build-cross-arch.sh` (needs synth >= 0.52.0).

## Measured

| driver | ARM cortex-m3 | RISC-V rv32imc |
|---|---|---|
| wdg-thin | 6 T, 2 U, 638 B | 5 T, 2 U — 1 of 6 skipped |
| gpio-thin | 5 T, 2 U, 502 B | 5 T, 2 U — complete |
| i2c-thin | 7 T, 2 U, 954 B | 6 T, 2 U — 1 of 7 skipped |
| timer-thin | 5 T, 2 U, 204 B | 5 T, 2 U — complete |
| adc-thin | 8 T, 2 U, 736 B | 8 T, 2 U — complete |
| dac-thin | 8 T, 2 U, 642 B | 8 T, 2 U — complete |
| pwm-thin | 6 T, 1 U, 694 B | 6 T, 1 U — complete |
| uart-thin | 4 T, 3 U, 254 B | 4 T, 3 U — complete |

Six of eight drivers lower **completely** on both targets, with the same number of
defined functions and the same seam width on each. Deterministic: repeated runs are
byte-identical.

## Why the undefined-symbol count is the gate, not the byte count

`build-cross-arch.sh` fails if either object has **zero** undefined symbols. That is not
a stylistic preference — it is the failure mode this whole approach has to detect. Under
synth 0.49 the RISC-V backend silently dropped every function containing an imported
call, so `wdg-thin` emitted a 468 B object with 2 defined functions and *no* undefined
symbols. It looked like a smaller, cleaner result. It was a truncated one. An object
whose `gust:hal` calls have vanished has not been ported; it has been hollowed out.

So the invariant is: the mmio imports must still be there, unresolved, on every target.
That is what makes the native layer swappable while the wasm stays put.

## Honest boundary

- **Executed on ARM silicon only.** The ARM objects are validated on two dies —
  Cortex-M4 and Cortex-M3, `silicon/RESULTS-wdg-g474re.md` and
  `silicon/RESULTS-wdg-f100.md`. The RISC-V objects are **compiled and seam-correct;
  they have not been run.**
- **And they could not meaningfully be run as-is.** These drivers carry STM32 register
  maps (the IWDG at `0x4000_3000`, and so on). Lowering them for RV32 proves the
  *toolchain* crosses; pointing them at an ESP32-C3's peripherals would be nonsense
  without an ESP32 target model. A faithful RISC-V silicon result needs an RV32 target
  in `targets/`, which is not done.
- **Import-free logic already has real RISC-V silicon numbers.** `gust_mix` is measured
  on a physical ESP32-C3 at 1.839x native — see `esp32c3/RESULTS.md`. That lane is
  execution evidence; this table is lowering evidence. They are different claims.
- **Two functions still decline**, reported upstream as synth#882: `wdg_unlock` hits an
  unsupported `BrTable` in the RV32 selector (a declared gap, declining loudly), and
  `i2c_step` fails with `undefined label 'Lend0'`, which looks like a genuine emit
  defect rather than a missing feature.

## What changed to make this possible

synth#871 — filed from this repo on 2026-07-29 after the RV32 backend was found to
decline every seam-importing function — was fixed the same day in synth 0.52.0, which
emits `auipc`/`jalr` plus `R_RISCV_CALL_PLT` into a `.rela.text`. Before that release,
no thin-seam driver crossed at all.
