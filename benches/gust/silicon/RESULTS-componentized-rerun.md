# Componentized wdg-thin / adc-thin, re-observed on silicon (2026-09-18)

REQ-DRV-SILICON-001's criterion: a componentized driver is done when the object built
from it has been **observed driving hardware**, with the observed effect compared against
the pre-change baseline — not when it composes.

Both drivers are componentized today (`check-driver-components.py`, VER-DRV-COMPONENT-001
PASS): `wdg-thin` imports `gust:hal/mmio@0.1.0` and its committed object's undefined set is
exactly `['read32', 'write32']`; `adc-thin` the same. This is the hardware half.

Runs via `cargo xtask silicon run <board> <bin>` (gale#399): the verdict is the firmware's
own line, and a run that prints nothing is REFUSED, never a pass.

## Result

| driver | board | baseline | this run | verdict |
|---|---|---|---|---|
| wdg-thin | STM32F100 (VLDISCOVERY) | `RCC_CSR` 0x34000000, IWDGRSTF=1 (RESULTS-wdg-f100.md, 2026-07-29) | **0x24000000, IWDGRSTF=1** | PASS |
| wdg-thin | STM32G474 (NUCLEO-G474RE) | `RCC_CSR` 0x3c000000, IWDGRSTF=1 (RESULTS-wdg-g474re.md, 2026-07-22) | **0x24000000, IWDGRSTF=1** | PASS |
| adc-thin | STM32F100 | Vrefint **1646** raw, VDDA ≈ 2985 mV (RESULTS-f100.md, 2026-07-23) | **1644** raw, VDDA ≈ 2989 mV | PASS |

## The differences, and why they are not the drivers

**`RCC_CSR` differs in bits the HARNESS sets, not the driver.** Decoded:

| | 0x34000000 (F100 baseline) | 0x3c000000 (G474 baseline) | 0x24000000 (both, today) |
|---|---|---|---|
| IWDGRSTF (29) — *the driver's effect* | set | set | **set** |
| SFTRSTF (28) | set | set | — |
| BORRSTF (27) | — | set | — |
| PINRSTF (26) | set | set | set |

- **SFTRSTF** is absent today because the baselines started the sequence with a software
  reset (`mww AIRCR SYSRESETREQ`, the old `run-wdg-f100.sh`), which sets that flag. The
  runner reaches the same clean start with `reset halt`. Different route to boot 1, same
  boot 2.
- **BORRSTF** in the G474 baseline is a stale brown-out flag from a power-on earlier in
  that session; the runner clears the flags (`CSR | RMVF`) before boot 1, so nothing stale
  survives into this run.
- **IWDGRSTF is the bit the two-boot proof is about**, and it is identical. A driver that
  silently no-op'd its start (KR=0xCCCC) would never reset, boot 2 would never happen, and
  the firmware could not print its OK line.

**Vrefint differs by 2 LSB** (1646 → 1644, VDDA 2985 → 2989 mV), inside the firmware's own
1450..1780 acceptance band and within ADC noise / supply variation between sessions. The
internal channel was converted (EOC cleared=true); a channel that was not converted reads
near 0 or full scale, which is what the band exists to catch.

## Scope

- **Physical bench, not CI.** These runs need boards; CI has none. The oracle is the
  firmware's verdict line, and the matrix is re-runnable with
  `cargo xtask silicon matrix --bins gust_wdg_silicon,gust_adc_silicon`.
- **Two drivers, three board-runs.** gpio, timer, spi and uart remain emulation-gated on
  their own axis; `gust_breadth` exercises all four on F100 registers (gale#397), but that
  is a different oracle from this per-driver baseline comparison.
- Toolchain: varve layer 2026.09.3 (synth 0.65.0). Zephyr fork pinned by revision (gale#405).
