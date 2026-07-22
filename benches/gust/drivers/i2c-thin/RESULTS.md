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

## Close-out (demonstrator + gate + rivet)

The `gust_i2c` demonstrator + Renode content-gate (register-effect assertion over a
RAM-mapped I2C1 window, like `gust_spi`) and the rivet `REQ-DRV-I2C-001` /
`VER-DRV-I2C-001` artifacts are now in place — the same closure the SPI/WDG drivers got:

- **`gust_i2c_probe`** (qemu-semihosting, `benches/gust/src/bin/`) drives the dissolved
  `i2c-thin-cm3.o` on a `[u32;16]` RAM window: asserts `i2c_configure` writes CR2
  FREQ=0x8 / CCR=0x28 / TRISE=0x9 / CR1.PE, a read START writes CR1 = PE|START|ACK =
  0x501, over a 3-byte read the ack decision is [1,1,0] and CR1.STOP (0x201) is written
  EXACTLY on the last byte (ACK-all-but-last, observable at the register level),
  completeness only after N bytes, dup-START/off-phase-`addr_ack` reject, and
  `i2c_stop` → Idle. Emits `i2c-probe ALL OK`, exit 0. Non-vacuous: a dissolved no-op
  leaves the window 0 → config/start assertions FAIL.
- **`gust-i2c-renode`** (`renode-test/gust_i2c.{repl,robot}`, wired in `BUILD.bazel` +
  `.github/workflows/gust-renode.yml`) runs the identical transaction on a real STM32
  M3 model, content-gated per line on USART1 (i2c-config-ok … i2c-stop-ok).

**Honest dissolve-fidelity finding (step off-Active).** The dissolved `i2c_step` does
NOT early-return the fault sentinel for a non-Active state the way the pure source
`step` returns `Err(WrongPhase)` — it enters its SR1 poll unconditionally and, on a
plain RAM window whose SR1 never advances, busy-waits. So the probe/gate exercise step
only on genuinely Active states (the ACK-all-but-last read); step's off-Active phase
gate is covered by the Kani `p4_phase_gating` SOURCE proof, and the native reject paths
are demonstrated via `i2c_start` (dup-START busy) and `i2c_addr_ack` (off-phase)
instead. This is a wasm→native divergence in a reject path, consistent with the
differentially-trusted (not proven-equivalent) dissolve; the safe polled path
(Active reads/writes with the peripheral advancing SR1) is unaffected.
