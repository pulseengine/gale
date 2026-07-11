# wdg-thin — verified thin-seam IWDG (independent watchdog) driver (gust:hal)

The 8th verified thin-seam iodev (after GPIO/timer/SPI/UART/I2C/ADC/DAC). The whole
STM32F1 IWDG key-sequence + lifecycle — 0x5555 unlock / PR+RLR config / 0xCCCC start /
0xAAAA refresh — in verified wasm, importing ONLY `gust:hal/mmio` (read32/write32):
**zero new TCB atoms**.

This is the **module-level hardware backstop** the partition-scheduler Health Monitor
design (gale#63) names: if the verified HM/switch core itself hangs, it stops servicing
the IWDG and the hardware forces a reset → fail-to-safe. So this driver's own
correctness is part of that safety argument.

## Distinctive property: cannot-un-start (Kani-proven)

Once the watchdog is started (0xCCCC) it can **never be disabled in software** — only a
system reset stops it. A watchdog you can accidentally turn off is worthless. The FSM
provides **no** disable transition, and `p2_cannot_un_start` proves that applied to a
Running watchdog *every* provided transition either keeps it Running (refresh) or is
rejected without mutating — there is no software path out of Running. Companion
invariants: config registers are **write-protected** until the 0x5555 key unlocks them
(`p1`/`p7`), start is Configured-only and one-way (`p4`), and a refresh only has effect
once Running (`p3`).

## Measured

- **Dissolve (loom 1.1.18 inline → synth 0.40.0 --target cortex-m3 --all-exports
  --relocatable): `wdg-thin-cm3.o` = text 660 / data 0 / bss 0 → 0 SRAM.** Scalar
  packed-u32 FSM (phase[31:30] · prescaler[14:12] · reload[11:0]) crosses the seam with
  no pointer; table-free config (pure bit arithmetic, no `.rodata` linmem). Imports are
  exactly `env.mmio_read32` / `env.mmio_write32` — the only undefined symbols.
- **Kani: 7/7 harnesses verified, 0 failures** — p1 write-protection · p2 cannot-un-start ·
  p3 refresh-only-running · p4 start-once · p5 config-bounds · p6 pack-roundtrip ·
  p7 unlock-gates-config. `cargo kani` (kani 0.67.0).

## Build

    cargo build --release --target wasm32-unknown-unknown   # needs .cargo/config --allow-undefined
    loom optimize <wasm> --passes inline | synth compile --target cortex-m3 --all-exports --relocatable

## Follow-on (not in this PR)

A `gust_wdg` demonstrator + Renode content-gate (assert the KR key sequence + that no
software write clears the running state) and a rivet `VER-DRV-WDG-001` artifact. Ties
into the partition-scheduler Health Monitor (gale#63) as the HW fail-to-safe backstop.
