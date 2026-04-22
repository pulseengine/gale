# `engine_control` — interrupt-driven benchmark

Simulates a crank-position sensor firing at the rate of a 4-stroke ICE
across 1,000–10,000 RPM. Each simulated crank-degree interrupt runs a
pure-C engine-control algorithm (spark advance + fuel duration table
lookups, coolant correction, knock retard), then hands off the result
through the Zephyr primitive chain Gale replaces.

Two timings are measured per interrupt:

| Segment | What it covers | Should differ gale vs baseline? |
|---|---|---|
| `algo` | `control_step()` — table lookups + small math, pure C | **No** — identical C in both builds |
| `handoff` | `ring_buf_put` + `k_sem_give` | **Yes** — Gale's verified primitives |

The `algo` identity across builds is the integrity check for the
measurement. The `handoff` delta is the engineering claim.

## Building

```sh
export ZEPHYR_BASE=/path/to/zephyr
export ZEPHYR_SDK_INSTALL_DIR=/path/to/zephyr-sdk-1.0.1
export GALE_ROOT=/path/to/gale

# Baseline — stock Zephyr primitives
west build -b qemu_cortex_m3 -d /tmp/engine-baseline \
  -s $GALE_ROOT/benches/engine_control

# Gale — verified Rust primitives swapped in
west build -b qemu_cortex_m3 -d /tmp/engine-gale \
  -s $GALE_ROOT/benches/engine_control \
  -- -DZEPHYR_EXTRA_MODULES=$GALE_ROOT \
     -DOVERLAY_CONFIG=$GALE_ROOT/benches/engine_control/prj-gale.conf
```

## Running

```sh
west build -d /tmp/engine-baseline -t run 2>&1 | tee /tmp/engine-baseline.csv
west build -d /tmp/engine-gale     -t run 2>&1 | tee /tmp/engine-gale.csv

python3 compare.py /tmp/engine-baseline.csv /tmp/engine-gale.csv \
  > comparison.md
```

## Sample output (QEMU qemu_cortex_m3)

12 MHz emulated cycle counter, 150 samples across 5 RPM steps:

| Metric | Baseline | Gale | Δ |
|---|---|---|---|
| `algo` mean | 79 cyc (6.6 μs) | 78 cyc (6.5 μs) | identical ✓ |
| `handoff` mean | 320 cyc (26.7 μs) | 283 cyc (23.6 μs) | **−11.6%** |
| `handoff` min | 220 cyc | 198 cyc | −10.0% |
| `handoff` max | 418 cyc | 393 cyc | −6.0% |

The algorithm-only timing is identical to 1-cycle noise (expected;
same C binary). Gale's verified primitive chain is consistently
faster and has a tighter max — the kind of bounded-latency property
formal verification should deliver.

Absolute QEMU numbers are not representative of hardware (QEMU
emulates cycles rather than reflecting board timing); the comparison
between the two builds under identical QEMU conditions is what's
meaningful.

## What the benchmark exercises

- `k_timer` (timer ISR → `crank_isr`)
- `control_step()` (pure C, in ISR context)
- `ring_buf_put` (ring-buffer with internal `k_spinlock`)
- `k_sem_give` (ISR → thread handoff)
- `k_sem_take` (thread drains sem)
- `ring_buf_get` (reader-side drain)

All of which have Gale-verified replacements when
`CONFIG_GALE_KERNEL_{SEM,TIMER,SPINLOCK,SPINLOCK_VALIDATE}=y`.

## Knobs

Edit `src/main.c`:
- `TOTAL_SAMPLES` — at default 150, a QEMU run takes ~60s. On real
  hardware you can bump to 10k+ for statistical power.
- `sweep[]` — the RPM schedule. Each step runs for `samples` interrupts
  at `rpm`.
- `HISTOGRAM_BUCKETS` — log2-scale buckets; 32 covers 1 cycle to 2³²
  cycles.

## Limitations (first version)

- QEMU cycle counter is deterministic per run but not realistic vs real
  hardware. Treat absolute ns numbers as simulator artifacts.
- The sweep driver uses `k_msleep(10)` for progress polling — hides
  small timing differences. For a precise run, make `sweep_driver` a
  tighter busy-waiter.
- No multi-run statistical analysis (Mann-Whitney / confidence
  intervals). The `compare.py` script reports deltas but doesn't test
  significance.
- Only single-CPU. SMP would need per-CPU histograms.

All of the above are tractable extensions; this version exists to
prove the infrastructure.
