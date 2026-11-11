# gust scheduler benchmark

Measures per-operation cost of the kiln-async scheduler that backs gust, on Cortex-M3.

## Method
- **Local (deterministic, fast):** `./run-bench.sh` — qemu `lm3s6965evb` with `-icount`
  (instruction-driven clock). Timing = SysTick current-value delta → deterministic and
  **instruction-proportional**. qemu's `lm3s6965evb` does not model the DWT cycle counter,
  so absolute units are SysTick "ticks", not cycles — use them for **scaling/regression**.
- **True Cortex-M3 cycles:** `renode/gust_bench_f100.robot` on the STM32F100
  (`stm32vldiscovery`) reads `ExecutedInstructions`. M3 has no cache/branch-predictor, so
  instruction count ≈ cycles — this is the fuel→cycles calibration source.

## Results (qemu -icount, `Scheduler<8,8,4,2,2>`)
| op | ticks/op | scaling | note |
|---|---|---|---|
| `poll_round` | **3.10** | **O(1)** — identical at 1/2/4/8 ready tasks | one task dispatched per poll |
| `spawn` | 3.13 | — | Spawned→Ready FSM + table insert + ready push |

**Key result:** per-poll cost is **constant in the ready-set size** — the scheduler dispatches
exactly one task per `poll_round`, so the superloop cost does not grow with task count. This is
the property the WCET argument needs (bounded per-round work).

**Footprint** (same image): `Scheduler<8,8,4,2,2>` = 408 B; `<6,6,4,2,2>` = 376 B (scales with
`NTASK`); kernel ~5 KB flash + ~0.4 KB RAM working-set — well inside the F100's 8 KB SRAM.

## Pending
Absolute Cortex-M3 cycle/WCET number via the Renode robot (run in CI or with a local Renode
install). Per-op tick figures here are deterministic and good for regression in the meantime.
