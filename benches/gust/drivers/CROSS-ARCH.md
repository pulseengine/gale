# Cross-architecture: one wasm, two instruction sets (re-measured 2026-08-28, synth 0.58.0)

The thin-seam drivers are written once, built once to wasm, and lowered to **both** ARM
Cortex-M and RISC-V RV32 from that same input. This is the "change the tires, not the
car" claim reduced to something checkable: the driver does not change, the native
residual under it does.

## Measured (re-measured 2026-08-28, synth 0.58.0)

Reproduce: `python3 benches/gust/drivers/check-cross-arch.py`.

| driver | imports | ARM cortex-m3 | RISC-V rv32imc |
|---|---|---|---|
| adc-thin | `read32 write32` | 45 T, 2 U — complete | 26 T, 3 U — 1 of 27 skipped, **1 dangling** |
| can-thin | `read32 write32` | 34 T, 2 U — complete | 19 T, 3 U — 1 of 20 skipped, **1 dangling** |
| dac-thin | `read32 write32` | 43 T, 2 U — complete | 24 T, 3 U — 1 of 25 skipped, **1 dangling** |
| gpio-thin | `read32 write32` | 30 T, 2 U — complete | 17 T, 3 U — 1 of 18 skipped, **1 dangling** |
| hm-thin | `(none)` | 19 T, 0 U — complete | 12 T, 1 U — 1 of 13 skipped, **1 dangling** |
| i2c-thin | `read32 write32` | 38 T, 2 U — complete | 21 T, 3 U — 1 of 22 skipped, **1 dangling** |
| mpu-thin | `mpu-write` | 27 T, 1 U — complete | **no object** — synth exits non-zero (#952) |
| pwm-thin | `write32` | 36 T, 1 U — complete | 21 T, 2 U — 1 of 22 skipped, **1 dangling** |
| spi-thin | `read32 write32` | 34 T, 2 U — complete | 19 T, 3 U — 1 of 20 skipped, **1 dangling** |
| switch-thin | `ctx-resume ctx-save region-swap` | 41 T, 3 U — complete | 17 T, 10 U — 13 of 30 skipped, **10 dangling** |
| timer-thin | `read32 write32` | 30 T, 2 U — complete | 17 T, 3 U — 1 of 18 skipped, **1 dangling** |
| uart-thin | `poll read32 write32` | 27 T, 3 U — complete | 16 T, 4 U — 1 of 17 skipped, **1 dangling** |
| wdg-thin | `read32 write32` | 35 T, 2 U — complete | 20 T, 3 U — 1 of 21 skipped, **1 dangling** |

**ARM: 13 of 13 lower completely**, and each object's undefined set is *exactly* the
set of names its wasm imports — no leak, no truncation. That half of the claim holds.

**RISC-V: 0 of 13.** Twelve emit an object that cannot be linked, because synth's RV32
selector skips a function it cannot select and emits the call site anyway, leaving a
dangling `synth_func_N`. The thirteenth (`mpu-thin`) emits nothing at all. This is not
new and is not a regression: synth 0.52.0 — the version the previous version of this
table cited — produces byte-for-byte the same result on today's sources, as does
0.60.0, the latest release. `ld.lld` refuses the objects outright:

```
ld.lld: error: undefined symbol: synth_func_18
>>> referenced by out.o:(gust:hal/gpio@0.1.0#set)
```

Filed upstream as **synth#1102**, and **fixed on synth `main` as #1104** — not
yet in a release (0.60.0 is the latest tag), so the ledger stays until we can pin
a build that carries it.

**The fix corrected our framing, and it is worth stating plainly.** This report
described the defect as rv32-specific, on the strength of the ARM column being
clean. It was not: synth's fix found **Thumb-2 and A32 shipping the identical
dangling symbol at exit 0**, on a module that declines on those backends. The
guard was missing everywhere; the ARM objects here are clean only because the ARM
backend declines *nothing* on this corpus, so the broken path is never entered.

That distinction matters for how the ARM row above should be read. "13 of 13,
undefined set equals imports" is a true statement about **these objects**. It is
not evidence that the ARM lowering path is sound, and it should not be cited as
such.

## What the previous table said, and why it was wrong

It reported eight drivers, six of them "complete" on both targets, gpio-thin at
"5 T, 2 U — complete". That was measured before these drivers were componentized
(`wdg-thin` had 6 functions then; it has 21 now), so it describes sources that no
longer exist. It was not re-measured when the sources changed.

Two mechanisms kept it from being caught:

1. **The gate enumerated its drivers by hand.** `build-cross-arch.sh` carried
   `DRIVERS=(wdg-thin gpio-thin i2c-thin timer-thin adc-thin dac-thin pwm-thin
   uart-thin)` — eight names, while thirteen `*-thin` directories exist. can, hm,
   mpu, spi and switch were never gated.
2. **The rule was `undefined_count > 0`.** See below.

## Why `> 0` was the wrong rule

The old gate failed an object with **zero** undefined symbols, on the reasoning that
a vanished seam means a truncated object (the synth 0.49 shape, where the RISC-V
backend dropped every function containing an imported call).

That reasoning is right about the failure it names and blind to its neighbour. `> 0`
is satisfied by *any* undefined symbol — including a leaked one. Every RISC-V object
above carries a dangling `synth_func_N`, so every one of them counted as passing. The
gate was green because the objects were broken in a way it could not distinguish from
being correct.

`hm-thin` shows the other edge: it imports nothing, so a correct object has **zero**
undefined symbols. Under `> 0` a correct driver fails.

The rule is therefore neither `> 0` nor `== 0` but **equality**:

> the lowered object's undefined set must equal the set of names the wasm imports.

That is what `check-cross-arch.py` gates, and it is the same rule
`check-driver-components.py` already applies on its object axis. It is correct for a
driver importing nothing, for a driver whose seam is not `gust:hal` (`mpu-thin`,
`switch-thin`), and it rejects a leak, which `> 0` cannot.

The RISC-V leg is carried as an explicit known-defect ledger in that script rather
than skipped: if a driver starts crossing cleanly the gate **fails** until it is
removed from the ledger, so the list shrinks to empty instead of outliving the bug.

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
