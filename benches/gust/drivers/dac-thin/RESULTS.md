# dac-thin — verified thin-seam DAC driver (gust:hal)

The 7th verified thin-seam iodev (after GPIO/timer/SPI/UART/I2C/ADC). The whole
STM32F1 DAC software-triggered path — CR channel/trigger config and the
enable→load→trigger→output cycle — in verified wasm, importing ONLY `gust:hal/mmio`
(read32/write32): **zero new TCB atoms**.

## Distinctive property: glitch-free, trigger-gated output (Kani-proven)

Writing the data holding register (DHR) does NOT move the pin — the output register
(DOR) updates only when a trigger fires. So a control loop stages a new value and
publishes it atomically, never emitting a half-updated code. Getting it wrong
(publish on load, or drive an unmasked value) → a glitch on an actuator line. `load`
always lands `Loaded` (staged, pin unchanged); `Output` is reachable ONLY via
`trigger` from `Loaded`, which preserves the value. `p3_glitch_free` +
`p2_output_reflects_loaded` prove it over the whole input space, and `p1_range_clamp`
proves every commanded value is masked to a 12-bit code (no overrange).

## Measured

- **Dissolve (loom 1.1.18 inline → synth 0.40.0 --target cortex-m3 --all-exports
  --relocatable): `dac-thin-cm3.o` = text 686 / data 0 / bss 0 → 0 SRAM.** Scalar
  packed-u32 FSM (phase[31:30] · channel[29] · value[11:0]) crosses the seam with no
  pointer; table-free config (pure bit arithmetic, no `.rodata` linmem). Imports are
  exactly `env.mmio_read32` / `env.mmio_write32` — the only undefined symbols.
- **Kani: 7/7 harnesses verified, 0 failures** — p1 range-clamp · p2
  output-reflects-loaded · p3 glitch-free · p4 phase-gating · p5 disable-total ·
  p6 pack-roundtrip · p7 config-well-formed. `cargo kani` (kani 0.67.0).

  > Note: `p2` initially FAILED — the oracle caught a **proof-modeling gap** (not a
  > driver bug): the arbitrary `any_dac()` explored a `Loaded` state with `value >
  > 0xFFF`, which the ABI can never reach (every state crosses the seam as a packed
  > u32, so `unpack` masks `value` to 12 bits). Fixed by modeling `any_dac()` as the
  > seam-reachable state space (`value & VALUE_MASK`, `channel & 1`) — faithful to what
  > `unpack` produces; the range property on the *commanded* value is tested separately
  > in p1 with an unconstrained input.

## Build

    cargo build --release --target wasm32-unknown-unknown   # needs .cargo/config --allow-undefined
    loom optimize <wasm> --passes inline | synth compile --target cortex-m3 --all-exports --relocatable

NOTE: rustc ≥1.97's rust-lld errors on undefined wasm cdylib symbols; the mmio
capability externs are emitted as wasm imports via `.cargo/config.toml`
(`--allow-undefined`), same as i2c-thin/adc-thin.

## Follow-on (not in this PR)

A `gust_dac` demonstrator + Renode content-gate (assert DHR write does NOT change DOR
until the SWTRIGR write, then DOR == the loaded 12-bit code — the glitch-free property
on real registers) and a rivet `VER-DRV-DAC-001` artifact.

---

_Toolchain note: current pins are synth 0.49.0 / loom 1.2.0 (#208). The 0.49 regen
measured this driver's dissolved `.text` at **678 B** (was 686 B on synth 0.40.0,
above); register effects unchanged, 0-SRAM preserved._
