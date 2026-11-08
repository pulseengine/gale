# can-thin — verified thin-seam CAN (bxCAN) driver (gust:hal)

The 10th verified thin-seam iodev (after GPIO/timer/SPI/UART/I2C/ADC/DAC/WDG/PWM). The
whole STM32F1 bxCAN master path — BTR bit-timing config, the INRQ→INAK init handshake,
and TX-mailbox / RX-FIFO gating — in verified wasm, importing ONLY `gust:hal/mmio`
(read32/write32): **zero new TCB atoms**.

## Distinctive property: config-only-in-init (Kani-proven)

bxCAN silently ignores writes to the bit-timing register (BTR) unless the peripheral is
in Init mode (INRQ requested, INAK confirmed) — a config write in Sleep or Normal would
be a no-op that leaves a stale/default bit rate live, corrupting every frame on the bus.
`configure(phase)` is a pure function of the mode FSM, and `p1_config_only_in_init`
proves the write is accepted **iff** the phase is Init, over the whole input space.
Companion invariants: Normal is reachable **only** by passing through Init
(`p3_normal_only_via_init`, so bit-timing always had a valid configuration window),
`enter_init` is total (`p2_enter_init_total`, requesting init is never rejected), and TX
(`p4`) / RX-release (`p5`) are both gated on Normal.

## Measured

- **Dissolve (loom 1.2.0 inline → synth 0.43.0 --target cortex-m3 --all-exports
  --relocatable): `can-thin-cm3.o` = text 804 / data 0 / bss 0 → 0 SRAM.** Scalar state
  (phase packed into the low 2 bits of a u32 — simpler than i2c's triple, no
  auxiliary count/direction to carry); table-free config (pure bit arithmetic, no
  `.rodata` linmem). `arm-none-eabi-nm` confirms the undefined-symbol set is EXACTLY
  `mmio_read32` / `mmio_write32` (zero new TCB atoms). Single-component dissolve (no
  meld fuse) — not affected by the wide-buffer-copy issue tracked in synth#757.
- **Kani: 7/7 harnesses verified, 0 failures** — p1 config-only-in-init ·
  p2 enter-init-total · p3 normal-only-via-init · p4 tx-requires-normal ·
  p5 rx-requires-normal · p6 btr-well-formed · p7 pack-roundtrip. `cargo kani`
  (kani 0.67.0).

## Build

    cargo build --release --target wasm32-unknown-unknown   # needs .cargo/config --allow-undefined
    loom optimize <wasm> --passes inline | synth compile --target cortex-m3 --all-exports --relocatable

NOTE: rustc ≥1.97's rust-lld errors on undefined wasm cdylib symbols; the mmio
capability externs are emitted as wasm imports via `.cargo/config.toml`
(`--allow-undefined`), same as the other thin-seam drivers.

## Follow-on (not in this PR)

A `gust_can` demonstrator + Renode content-gate (assert BTR is written only inside the
INRQ/INAK window, and that TXRQ/RFOM0 are only ever set live-gated on TME0/FMP0) —
the same closure the other thin-seam drivers got. Discharges rivet `VER-DRV-CAN-001`
(artifacts/gust_can_driver.yaml).

---

_Toolchain note: current pins are synth 0.49.0 / loom 1.2.0 (#208). The 0.49 regen
measured this driver's dissolved `.text` at **796 B** (was 804 B on synth 0.43.0,
above); register effects unchanged, 0-SRAM preserved._
