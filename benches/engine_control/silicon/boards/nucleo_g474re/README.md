# NUCLEO-G474RE — silicon-anchor board notes

## Hardware

- **Board:** STMicroelectronics NUCLEO-G474RE
- **MCU:** STM32G474RET6 (Cortex-M4F + FPU + DSP, 170 MHz)
- **Memory:** 512 KB Flash, 128 KB RAM
- **Cycle counter:** DWT_CYCCNT (same as Cortex-M4F on `stm32f4_disco`)
- **Programmer:** integrated ST-Link/V3E over USB; exposes virtual
  COM port for stdout
- **Upstream Zephyr support:** `nucleo_g474re` (already in the tree)

## Why this board for the anchor

Cortex-M4F + FPU at 170 MHz is the closest production-shape silicon
to the simulated `stm32f4_disco` (also Cortex-M4F + FPU at 168 MHz).
The architectural variables held constant between the synthetic and
silicon measurements are:

- ARMv7E-M instruction set (Thumb-2)
- DWT_CYCCNT cycle counter (same width, same definition)
- 3-stage in-order pipeline
- Single-cycle MUL, hardware DIV, single-precision FPU

What differs:

- Real cache effects (none on Cortex-M4 — no D-cache; flash
  prefetch buffer behavior visible)
- Real bus arbitration with non-existent peripherals on this bench
  (negligible — no DMA, no peripheral activity)
- Clock 170 vs 168 MHz (1.2% — accountable directly)

So the cycle ratio `silicon / renode` for `algo` and `handoff`
should be near 1.0 in steady state. Anything materially off is
information about Renode's cycle model, not about the silicon.

## Connection

USB cable from NUCLEO USB connector (CN1) to host. The ST-Link
virtual COM port appears as:

- macOS: `/dev/cu.usbmodem*`
- Linux: `/dev/ttyACM0`

Zephyr's default for this board uses LPUART1 for stdout, exposed
through ST-Link.

## Programming

`west flash` from a build directory works out of the box:

```sh
west flash -d /tmp/eng-nucleo-baseline
```

Default backend is OpenOCD. To force pyOCD:

```sh
west flash -d /tmp/eng-nucleo-baseline --runner pyocd
```

## Clock / cycle counter notes

On the G4 family, `k_cycle_get_32()` returns `SCB_DWT->CYCCNT`
directly, same as on F4. `sys_clock_hw_cycles_per_sec()` returns
the bus clock the cycle counter ticks at — verify this matches
170 MHz at runtime by reading the boot banner before relying on
absolute ns conversions.

## Known issues

None yet — populate as captures happen.
