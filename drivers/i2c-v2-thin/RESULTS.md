# i2c-v2-thin — gust:bus i2c-master over the STM32 I2C v2 block

The bus driver for wohl's air node (gale#511, seam `gale-wohl/air-sensor-read`).
Targets the I2C v2 block that WB55, G0 and G4 share (`i2c v2` in the pinned
stm32-data chip files for STM32WB55RG, STM32G474RE and STM32G031K8). Imports only
`gust:hal/mmio` (read32, write32). Exports:

- `gust:bus/i2c-master@0.1.0`: `begin`, `xfer`, `end` in the encoding
  `wit/bus/gust-bus.wit` defines.
- `gust:i2c-v2/setup@0.1.0` (`wit/setup.wit`): `configure(timingr)`. TIMINGR depends
  on this IP and on the board's I2C kernel clock, so it stays out of the portable
  interface. Board bring-up enables the peripheral clock, routes the pins and calls
  `configure` before the first `begin`. The driver hard-codes no TIMINGR value.

The I2C1 base comes from `targets/generated/gust_target_<soc>.rs`, selected by one
cargo feature (`stm32wb55` by default, `stm32g474`, `stm32g031`).

## Why this is not a port of i2c-thin

The F1 block makes the master drive ACK, NACK and STOP byte by byte, and
`drivers/i2c-thin` proves that ACK-all-but-last decision. On the v2 block the
master writes the whole transfer into CR2 once (SADD, DIR, NBYTES, AUTOEND, START),
and the hardware NACKs the last read byte and issues STOP. What the driver must get
right is therefore the CR2 word and the byte count, and those are what is proven.

## Measured (local, 2026-11-14)

- **Kani 0.67.0: 9 of 9 harnesses verified, 0 failures.** k1 begin contract and
  every CR2 field; k2 RELOAD clear, AUTOEND set, NBYTES == count; k3 exactly
  `count` moves, then Complete, which `xfer` refuses; k4 states < 0x800, errors
  >= 0xFFFF_FF00, read results keep the state in bits 0..=15; k5 error flags win
  over the data flag; k6 decode is total; k7 the end plan is total; k8 TIMINGR
  reserved bits refused; k9 `end` reports a NACK of the final written byte.
- **Negative controls, each caught by its harness (exit 1):** RELOAD set (k2),
  NBYTES = count - 1 (k1), data flag checked before error flags (k5), NACK check
  removed from `end` (k9).
- **Dissolve, pinned loom 1.4.1 and synth 0.89.0:**
  `loom optimize --passes inline` then
  `synth compile --target cortex-m3 --all-exports --relocatable --embedder-data-init --embedder-global-init`:
  text 2578 / data 0 / bss 0. Undefined symbols: exactly `read32`, `write32`.
  The wasm carries the 16-byte `.rodata` segment that wit-bindgen 0.52 emits in
  every driver (uart-comp has the identical bytes); no code reads it, and the
  object has 0 r9 and 0 0x100000 references, which `drivers/check-cross-arch.py`
  checks.

## What is not shown

- Kani covers the pure decision functions (`plan_begin`, `classify`,
  `classify_end`, `decode`, `advance`, `plan_end`, `configure_ok`). The register
  shell that calls them (bounded polling, `recover`, `soft_reset`) is unproven and
  has not run on Renode or silicon.
- The hardware behaviour k2 relies on (NACK of the last byte and STOP when NBYTES
  is reached with RELOAD = 0 and AUTOEND = 1) is assumed from the reference manual.
- The 7-bit address placement in SADD[7:1] is not in the register data; it was
  cross-checked against one HAL implementation.
- Waits are bounded by iteration count (`SPIN_MAX` = 100000), not by time.
- A slave that holds SDA low after an abandoned read needs bus recovery (clocking
  SCL through GPIO), which this driver does not do.
- The driver Kani harnesses do not run in CI (gale#509).

## Build and check

    cargo build --release --target wasm32-unknown-unknown
    cargo kani
