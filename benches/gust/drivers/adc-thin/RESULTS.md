# adc-thin — verified thin-seam ADC driver (gust:hal)

The 6th verified thin-seam iodev (after GPIO/timer/SPI/UART/I2C). The whole STM32F1
ADC single-conversion path — SMPR sample-time + SQR regular-sequence config, and the
enable→start→EOC→read conversion cycle — in verified wasm, importing ONLY
`gust:hal/mmio` (read32/write32): **zero new TCB atoms**.

## Distinctive property: read-after-EOC, exactly-once (Kani-proven)

The data register may be read only after End-Of-Conversion, and reading it consumes
the sample (EOC clears on read). Read early → a stale/garbage sample; let the ADC
free-run (CONT=1) or read twice → a torn value silently used as a control input.
`read(state, raw)` is accepted only from `Complete` and lands `Ready` (never
`Converting`), so one `start` yields exactly one `EOC` → one `read` → back to Ready —
no free-run. `p2_read_after_eoc` + `p3_single_shot` prove it over the whole input
space; `sample` is shown to change ONLY on `read` (begin/complete carry it through
untouched — no torn mid-conversion value).

## Measured

- **Dissolve (loom 1.1.18 inline → synth 0.40.0 --target cortex-m3 --all-exports
  --relocatable): `adc-thin-cm3.o` = text 754 / data 0 / bss 0 → 0 SRAM.** Scalar
  packed-u32 FSM (phase[31:30] · channel[29:25] · sample[11:0]) crosses the seam with
  no pointer; table-free config (pure bit arithmetic, no `.rodata` linmem). Imports
  are exactly `env.mmio_read32` / `env.mmio_write32` — the only undefined symbols.
- **Kani: 7/7 harnesses verified, 0 failures** — p1 channel-bounds · p2 read-after-EOC ·
  p3 single-shot · p4 phase-gating · p5 disable-total · p6 pack-roundtrip ·
  p7 config-well-formed. `cargo kani` (kani 0.67.0).

## Build

    cargo build --release --target wasm32-unknown-unknown   # needs .cargo/config --allow-undefined
    loom optimize <wasm> --passes inline | synth compile --target cortex-m3 --all-exports --relocatable

NOTE: rustc ≥1.97's rust-lld errors on undefined wasm cdylib symbols; the mmio
capability externs are emitted as wasm imports via `.cargo/config.toml`
(`--allow-undefined`), same as i2c-thin.

## Follow-on (not in this PR)

A `gust_adc` demonstrator + Renode content-gate (assert the CR2 ADON/SWSTART writes
and the DR-read-after-EOC ordering over a RAM-mapped ADC window, like `gust_spi`) and
a rivet `VER-DRV-ADC-001` artifact — the same closure the SPI driver got.

---

_Toolchain note: current pins are synth 0.49.0 / loom 1.2.0 (#208). The 0.49 regen
measured this driver's dissolved `.text` at **740 B** (was 754 B on synth 0.40.0,
above); register effects unchanged, 0-SRAM preserved._
