# Three dies, one session (2026-08-04)

All three architectures the gust work claims were exercised **live, back to back,
with all three probes attached at once** — the presentation fallback capture. Each
log here is the real console output, ANSI-stripped and filtered to the result
lines; nothing is spliced between boards.

| # | architecture | board | what ran | result |
|---|---|---|---|---|
| 1 | Cortex-M4 (thumbv7em) | NUCLEO-G474RE, ST-LINK/V3 | dissolved `wdg-thin` arms the real IWDG | `RCC_CSR=0x34000000`, `IWDGRSTF=1` — hardware reset fired |
| 2 | Cortex-M3 (thumbv7m) | STM32F100 VLDISCOVERY, ST-LINK/V1 over a Pi | same `.o`, second die | `0x14000000 → 0x34000000`, `IWDGRSTF=1` |
| 3 | RISC-V RV32IMC | ESP32-C3 rev v0.4 | dissolved `gust_mix` vs native LLVM | 271 vs 499–500 milliticks → **1.839×**, correctness IDENTICAL, `mismatch=0` |

## What each leg does and does not show

**Legs 1 and 2 are the same happy path on two dies.** Boot 1 arms the watchdog
through the dissolved driver and stops refreshing it; the hardware resets the chip;
boot 2 reads the flag. It evidences that the dissolved object drives real hardware
to the real effect. It does **not** evidence `p2_cannot_un_start` — the firmware
never attempts an un-start, so no rejection path is exercised on silicon. That
property is a source-level Kani proof and stays one.

**Leg 3 is a reproduction, not a new number.** It re-flashes the **committed synth
0.40.0 object**; the current toolchain pin is 0.49.0. It reproduces the 2026-07-11
row and does not close that caveat — see `../esp32c3/RESULTS.md`, which also records
why the re-dissolve is blocked (the `gust_mix` wasm input is not in this repo).

**The two capture shapes differ, and the difference matters.** The F100 run is a
single unspliced openocd session that holds SWD across the reset, so boot 1 →
reset → boot 2 is one continuous capture. The G474 run is **two probe-rs
invocations**, because the watchdog reset kills the probe-rs session — visible in
`g474-wdg.log` as `Error: Exception` ending the first invocation. Both boots are
shown; they are not presented as one take.

## Session notes

- `Error: SRST error` in `f100-wdg.log` is openocd probing the ST-LINK/V1 reset
  line. Non-fatal — the run proceeds and completes.
- `run-wdg.sh` needed a fix to produce this bundle at all: with the ESP32-C3 also
  plugged in, probe-rs sees two probes, goes interactive, and dies with
  `Failed to parse probe index`. It now auto-selects the ST-LINK (override with
  `PROBE=VID:PID:SERIAL`). The all-three-dies-at-once case is precisely the one
  that broke it.
