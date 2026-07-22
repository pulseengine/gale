# wdg-thin — SILICON-VALIDATED on NUCLEO-G474RE (2026-07-22)

The dissolved (wasm → loom → synth `--target cortex-m3 --relocatable`) **wdg-thin**
IWDG driver, driving the **real** STM32 hardware watchdog on a physical
**NUCLEO-G474RE** (STM32G474, Cortex-M4, onboard ST-LINK/V3, flashed sudo-free via
probe-rs 0.31.0). The IWDG is register-identical across the STM32 line (base
0x4000_3000, KR/PR/RLR key sequence), so the F1-authored cortex-m3 `.o` programs the
G4 watchdog verbatim (thumbv7m ⊂ thumbv7em).

Firmware: `benches/gust/src/bin/gust_wdg_silicon.rs` (two-boot self-checking).
Reproduce: `benches/gust/silicon/run-wdg.sh`.

## Captured evidence

Boot 1 — the dissolved driver arms the real IWDG, then stops kicking it:

    gust-wdg-silicon: boot 1 (RCC_CSR=0x1c000000, no prior WDG reset). Arming the REAL
    IWDG @0x40003000 via the dissolved wdg-thin driver (PR=5, RLR=0x123 ≈ 1.2 s)...
    gust-wdg-silicon: armed (last KR write=0x0000, is_running=1). NOT refreshing —
    expect a HARDWARE reset in ~1.2 s, after which boot 2 reads IWDGRSTF=1.

~1.2 s later the hardware IWDG fired a full chip reset (observed as a reset/exception
at the vector table right on schedule). Boot 2 (after the reset):

    gust-wdg-silicon OK: IWDG watchdog reset CONFIRMED on real G474RE silicon
    (RCC_CSR=0x3c000000, IWDGRSTF=1) — the dissolved wdg-thin driver armed the
    hardware watchdog and it fired the reset.
    Firmware exited successfully

`RCC_CSR=0x3c000000` has bit 29 (`IWDGRSTF`) set — the independent watchdog was the
reset source. A silently-no-op'd start (KR=0xCCCC) would never reset, so the test
cannot false-pass.

## Scope / honesty
- This validates the wdg driver's IWDG programming + the cannot-un-start effect
  **on real silicon** — the strongest evidence tier (above the qemu probe + Renode
  content-gate). The Kani proofs (7/7) remain the source-level guarantee; this shows
  the *dissolved object* drives real hardware to the real effect.
- Only the **IWDG** is register-portable F1→G4. adc/dac/i2c/can/pwm use F1-specific
  register maps → faithful silicon needs an STM32F1 board (VLDISCOVERY) or a G4
  re-target; those remain qemu/Renode-validated for now.
