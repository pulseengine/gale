# wdg-thin — SECOND DIE: STM32F100 (VLDISCOVERY), Cortex-M3 (2026-07-29)

The **same** dissolved `wdg-thin-cm3.o` that fired the watchdog on the NUCLEO-G474RE
(Cortex-M4) also fires it on an **STM32F100 / Cortex-M3** — a different family, a
different die, and the architecture the object was actually compiled for
(`synth --target cortex-m3`). On the G4 it runs as thumbv7m ⊂ thumbv7em; here it runs
natively.

Nothing about the driver was rebuilt for this: the wasm, the dissolve, and the `.o`
are identical. Only `memory.x` (flash/RAM geometry) and the firmware's target feature
differ, and neither touches the driver.

## Captured evidence

Two consecutive runs, `wdg-f100-run{1,2}.log`, identical in outcome:

    gust-wdg-silicon: boot 1 on STM32F100 (RCC_CSR=0x14000000, no prior WDG reset).
    Arming the REAL IWDG @0x40003000 via the dissolved wdg-thin driver
    (PR=5, RLR=0x123 ≈ 1.2 s)...
    gust-wdg-silicon: armed (last KR write=0x0000, is_running=1). NOT refreshing —
    expect a HARDWARE reset in ~1.2 s, after which boot 2 reads IWDGRSTF=1.
    gust-wdg-silicon OK: IWDG watchdog reset CONFIRMED on real STM32F100 silicon
    (RCC_CSR=0x34000000, IWDGRSTF=1) — the dissolved wdg-thin driver armed the
    hardware watchdog and it fired the reset.

`RCC_CSR` 0x14000000 → 0x34000000: bit 29 (`IWDGRSTF`) set by the hardware.

## Why this run is better evidence than the G474RE one

- **One continuous session, not two.** The G474RE capture is two `probe-rs`
  invocations, because the watchdog reset kills the probe-rs session — boot 1 and
  boot 2 had to be reported as separate takes. Here openocd holds the SWD session
  across the reset, so boot 1 → hardware reset → boot 2 is a **single unspliced
  capture**. The reset is observed in-line, not inferred from a reconnect.
- **The reset flags are cleared by the harness immediately before the run**
  (`mww 0x40021024 0x01000000`, RMVF), and the CPU is started with a software reset
  (`SYSRESETREQ`), which sets `SFTRSTF` — *not* `IWDGRSTF`. So a set `IWDGRSTF` at
  boot 2 cannot be stale.
- **Reproduced twice**, back to back, byte-identical outcome.

## Scope / honesty

- This is a **second die** for the same happy path, not a new property. It evidences
  portability of the dissolved object and reproduces the arm→fire effect. It does
  **not** evidence `p2_cannot_un_start` — the firmware still never attempts an
  un-start (see `RESULTS-wdg-g474re.md`).
- **~1.2 s remains the configured timeout**, computed from PR=5 / RLR=0x123 against a
  nominal ~32 kHz LSI. It is not measured here either; the LSI is spec'd loose and the
  F1 and G4 LSIs are different parts.
- Portability holds here because the **IWDG is register-identical** across the STM32
  line. It does not generalise: adc/dac/i2c/can/pwm use family-specific register maps.
- The cross-*architecture* claim is narrower still. The same wasm dissolves to
  RISC-V only for import-free modules; `synth` 0.49.0 cannot yet emit a relocatable
  RISC-V object for a module with imports (`external call without relocation table`),
  so no thin-seam driver dissolves to RISC-V today. Filed upstream as synth#871.
  What is demonstrated here is **one wasm → one Cortex-M object → two different STM32
  dies**.

## Reproduce

    benches/gust/silicon/run-wdg-f100.sh          # needs an ST-LINK + openocd

## Re-validated under synth 0.52.0 (2026-07-29)

Repeated with the 0.52.0-regenerated object (638 B, symbol shape unchanged), again as
one unspliced openocd session across the reset:

    gust-wdg-silicon: boot 1 on STM32F100 (RCC_CSR=0x14000000, no prior WDG reset)...
    gust-wdg-silicon: armed (is_running=1). NOT refreshing...
    gust-wdg-silicon OK: IWDG watchdog reset CONFIRMED on real STM32F100 silicon
    (RCC_CSR=0x34000000, IWDGRSTF=1)

Both dies therefore hold under the new pin.

## Re-validated as a COMPONENT (2026-07-30)

Repeated on the second die with the componentized driver (`export gust:hal/wdg@0.1.0`,
seam now `read32`/`write32`), again as one unspliced openocd session across the reset:

    gust-wdg-silicon: boot 1 on STM32F100 (RCC_CSR=0x14000000, no prior WDG reset)...
    gust-wdg-silicon: armed (is_running=1). NOT refreshing...
    gust-wdg-silicon OK: IWDG watchdog reset CONFIRMED on real STM32F100 silicon
    (RCC_CSR=0x34000000, IWDGRSTF=1)

Both dies therefore hold with the driver behind a typed WIT interface.
