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
- This validates the wdg driver's IWDG **unlock → configure → lock → start** key
  sequence **on real silicon** — the strongest evidence tier (above the qemu probe +
  Renode content-gate). The Kani proofs (7/7) remain the source-level guarantee; this
  shows the *dissolved object* drives real hardware to the real effect.
- **NOT evidenced here: cannot-un-start.** An earlier revision of this file claimed
  this run validated "the cannot-un-start effect on real silicon". It does not, and the
  claim has been withdrawn. `gust_wdg_silicon.rs` arms the watchdog and then simply
  stops refreshing it; it never *attempts* an un-start, so nothing on silicon exercised
  a rejection path. `p2_cannot_un_start` is a source-level Kani property over the pure
  FSM and stays exactly that. What this run evidences is one happy path, once, on one
  die: the dissolved object emitted a key sequence the real IWDG accepted, and the
  hardware fired. (`RESULTS` also records a second session at
  `RCC_CSR=0x14000000 → 0x34000000`, so the effect has reproduced across runs. It has
  since also reproduced on a **second die of a different family** — the same `.o` on an
  STM32F100/Cortex-M3, twice, in one unspliced session: see `RESULTS-wdg-f100.md`.)
- The stated **~1.2 s** is the *configured* timeout computed from PR=5 / RLR=0x123
  against a nominal ~32 kHz LSI — it is not a measured interval, and the LSI is spec'd
  loose. The captured probe-rs log shows a much longer wall-clock gap before the
  session drops (host detection latency), so no timing claim should be read off it.
- Only the **IWDG** is register-portable F1→G4. adc/dac/i2c/can/pwm use F1-specific
  register maps → faithful silicon needs an STM32F1 board (VLDISCOVERY) or a G4
  re-target; those remain qemu/Renode-validated for now.

## Re-validated under synth 0.52.0 (2026-07-29)

The committed `wdg-thin-cm3.o` was regenerated on synth 0.52.0 (648 -> 638 B text,
symbol shape byte-identical) and the run above was repeated on the same board:

    gust-wdg-silicon: boot 1 on STM32G474 (RCC_CSR=0x14000000, no prior WDG reset)...
    gust-wdg-silicon: armed (is_running=1). NOT refreshing...
    [session drops — the watchdog resets the chip]
    gust-wdg-silicon OK: IWDG watchdog reset CONFIRMED on real STM32G474 silicon
    (RCC_CSR=0x34000000, IWDGRSTF=1)

Same effect, same flag, smaller object. The re-pin does not disturb this result.
