# i2c-thin — verified thin-seam I2C driver (gust:hal)

The 5th verified thin-seam iodev (after GPIO/timer/SPI/UART). The whole STM32F1 I2C
master path — CR2 FREQ + CCR + TRISE timing config, and the START→address→data→STOP
transaction — in verified wasm, importing ONLY `gust:hal/mmio` (read32/write32):
**zero new TCB atoms**.

## Distinctive property: ACK-all-but-last (Kani-proven)

An I2C master reading N bytes must ACK bytes 1..N−1 and NACK byte N (so the slave
releases SDA and the master can STOP). Getting it wrong hangs the bus or drops a
byte. `ack_byte(state)` is a pure function of the FSM state, and `p3_ack_all_but_last`
proves it holds over the whole input space.

## Measured

- **Dissolve (loom 1.1.18 inline → synth 0.40.0 --target cortex-m3 --all-exports
  --relocatable): `i2c-thin-cm3.o` = text 992 / data 0 / bss 0 → 0 SRAM.** Scalar
  packed-u32 FSM (phase[31:30] · read[29] · remaining[28:0]) crosses the seam with no
  pointer; table-free config (pure bit arithmetic, no `.rodata` linmem).
- **Kani: 7/7 harnesses verified, 0 failures** — p1 exclusive-bus · p2 no-lost-byte ·
  p3 ACK-all-but-last · p4 phase-gating · p5 stop-frees-bus · p6 pack-roundtrip ·
  p7 config-well-formed. `cargo kani` (kani 0.67.0).

## Build

    cargo build --release --target wasm32-unknown-unknown   # needs .cargo/config --allow-undefined
    loom optimize <wasm> --passes inline | synth compile --target cortex-m3 --all-exports --relocatable

NOTE: rustc ≥1.97's rust-lld errors on undefined wasm cdylib symbols; the mmio
capability externs are emitted as wasm imports via `.cargo/config.toml`
(`--allow-undefined`). The other thin-seam drivers (spi/gpio/timer/uart) predate this
and need the same one-line config to rebuild (their committed `.o`s still link fine).

## Follow-on (not in this PR)

A `gust_i2c` demonstrator + Renode content-gate (register-effect assertion over a
RAM-mapped I2C window, like `gust_spi`) and a rivet `VER-DRV-I2C-001` artifact — the
same closure the SPI driver got.
