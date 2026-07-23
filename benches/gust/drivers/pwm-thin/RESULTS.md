# pwm-thin — verified thin-seam PWM driver (gust:hal)

The 9th verified thin-seam iodev (after GPIO/timer/SPI/UART/I2C/ADC/DAC/IWDG). The whole
STM32 advanced-timer PWM output path — PWM-mode config (CCMR PWM mode 1 + preload),
period (ARR) / duty (CCR) load, output + main-output enable — in verified wasm, importing
ONLY `gust:hal/mmio`: **zero new TCB atoms**.

This is the **actuator-output driver**. On the Pixhawk 6X-RT its failsafe *is* the
**4× failsafe-PWM** the cross-core Health Monitor (gale#63) trips on FMU loss / window
overrun, so its two safety properties are load-bearing.

## Distinctive properties (Kani-proven)

1. **duty ≤ period** (`p1_duty_clamp`): the compare value can never exceed the auto-reload,
   for any commanded duty/period — so no unintended 100% (full-throttle / hard-over)
   output. Every `set_duty` clamps.
2. **failsafe is total and latching** (`p2_failsafe_total_off`, `p3_failsafe_latches`):
   `failsafe` forces outputs off (clears MOE) from ANY state → Safe, and from Safe neither
   `start` nor `set_duty` is accepted — only an explicit `configure` re-arms. A tripped
   failsafe cannot be casually undone.

## Measured

- **Dissolve (loom 1.1.18 inline → synth 0.40.0 --target cortex-m3 --all-exports
  --relocatable): `pwm-thin-cm3.o` = text 728 / data 0 / bss 0 → 0 SRAM.** Scalar
  packed-u32 state (phase[31:30] · duty[15:0]; the period is a configure-time arg, not
  state); table-free config (pure bit arithmetic). The only undefined symbol is
  `env.mmio_write32` (PWM is a pure-output path — it never reads a register).
- **Kani: 7/7 harnesses verified, 0 failures** — p1 duty-clamp · p2 failsafe-total-off ·
  p3 failsafe-latches · p4 phase-gating · p5 config-well-formed · p6 pack-roundtrip ·
  p7 start-keeps-duty. `cargo kani` (kani 0.67.0).

## Build

    cargo build --release --target wasm32-unknown-unknown   # needs .cargo/config --allow-undefined
    loom optimize <wasm> --passes inline | synth compile --target cortex-m3 --all-exports --relocatable

## Follow-on (not in this PR)

A `gust_pwm` demonstrator + Renode content-gate (assert CCR ≤ ARR always, and that
`pwm_failsafe` clears MOE and no subsequent write re-enables output without a reconfigure)
+ rivet `VER-DRV-PWM-001`. Wires into the gale#63 Health Monitor as the actuator failsafe
output stage.

---

_Toolchain note: current pins are synth 0.49.0 / loom 1.2.0 (#208). The 0.49 regen
measured this driver's dissolved `.text` at **706 B** (was 728 B on synth 0.40.0,
above); register effects unchanged, 0-SRAM preserved._
