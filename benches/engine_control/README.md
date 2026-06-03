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

## Methodology — event stream, off-target statistics

Post-issue #25 (red-team audit — see
`docs/research/engine-bench-methodology-review.md`), the benchmark
emits **raw per-ISR events** and computes all statistics off-target.

On-target (firmware):
- The ISR measures `algo_cycles` (around `control_step`) and publishes
  a `crank_sample` into a SPSC ring buffer. After `ring_buf_put +
  k_sem_give` it publishes `handoff_cycles` into a side-channel slot
  keyed by seq.
- A reader thread drains the ring and emits one CSV line per sample:
  ```
  E,<seq>,<step>,<rpm>,<algo_cycles>,<handoff_cycles>
  ```
- **No** mean, min, max, or histogram is computed in firmware.

Off-target (`analyze.py`):
- Parses event lines, groups by RPM step, computes per-step median
  with a 2000-iteration bootstrap 95% CI on the median.
- Runs Mann-Whitney U (tie-corrected, normal-approximated p-value) to
  test whether baseline and Gale distributions differ.
- Reports pooled p50/p75/p95/p99/max for the handoff segment.
- Checks integrity: baseline and Gale algo medians must agree within
  10% (same C, same measurement path).

This replaces the in-firmware histogram+mean approach whose mean
divisor (reader `count`) diverged from the numerator (ISR event sum)
when the sweep truncated early, invalidating the published deltas.

## Framework overhead compensation

Every `algo_cycles` and `handoff_cycles` value emitted on the wire is
the raw measurement **minus** a constant `bench_overhead_cycles`,
measured at boot before any per-event timing begins. The
`measure_overhead()` routine in `src/main.c` runs

```c
start = k_cycle_get_32();
end   = k_cycle_get_32();
delta = end - start;
```

1000 times under `irq_lock`, sorts the deltas, and stores the
**median** as `bench_overhead_cycles`. That value is then subtracted
(saturating at 0) from every per-event count before it reaches the
CSV stream, so what's reported is the work between the cycle-counter
reads, not the cost of the cycle-counter reads themselves.

The compensation is **visible**: the measured value is emitted as a
metadata line `overhead_cycles,<value>` in the CSV header, preserved
into the artifact bundle, and surfaced in `analyze.py`'s report header
as "Overhead subtracted (cycles): baseline ...; gale ..." — a
reviewer can audit the subtraction and re-add it if they want the
raw numbers back.

This matches the upstream Zephyr 4.4 `ztest_bench` framework's `ctrl`
benchmark pattern (`subsys/testsuite/ztest/benchmark/`), which
measures and subtracts the cost of an `empty_function` call from
every reported result. Pre-compensation and post-compensation numbers
are **different measurements** — do not combine them in a single
comparison table.

## Scope and non-claims

See [SCOPE.md](SCOPE.md) for the explicit list of what this bench
measures and what it does NOT measure. That file is the source of
truth for any downstream copy (blog posts, reports). Do not embed
scope claims in published copy without first updating SCOPE.md.

## Silicon-anchor protocol

Renode is the CI workhorse; **silicon captures are manual**, periodic,
and recorded directly into the repo as immutable evidence. See
[`silicon/README.md`](silicon/README.md) for the procedure, board
notes, and the `capture.sh` wrapper. Per-board configs live under
`silicon/boards/`; recorded captures land under `silicon/runs/<dated>/`
with a manifest, the firmware ELF, and the tagged events CSV.

The first supported board is the NUCLEO-G474RE (STM32G474, Cortex-M4F
+ FPU, 170 MHz) — closest production-shape silicon to the
`stm32f4_disco` Renode target. The ratio `silicon_median /
renode_median` per RPM step is what the anchor establishes; once
consistent across multiple captures it can be cited as the
Renode-silicon multiplier.

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

The `run_qemu_bench.sh` wrapper builds both variants, runs each N
times (default N=1 for fast smoke; `-n 20` for manual statistical
runs), concatenates event streams, and invokes `analyze.py`:

```sh
# Fast sanity check (same as CI smoke)
bash benches/engine_control/run_qemu_bench.sh

# Statistical power run (~20× longer)
bash benches/engine_control/run_qemu_bench.sh -n 20
```

The analyzer writes a markdown report to stdout with per-step tables
and pass/fail assertions. Exit status is 0 only when all asserts
pass.

Manual invocation:

```sh
python3 analyze.py --baseline /tmp/engine-baseline/events.csv \
                   --gale     /tmp/engine-gale/events.csv \
                   --runs 1
```

## Output format

The analyzer emits:

```
# Engine-control benchmark — event-stream analysis

- Runs per variant: 1
- Baseline events: 150 (target 150, drops 0)
- Gale events:     150 (target 150, drops 0)
- Cycle counter:   12,000,000 Hz

## `algo` cycles — per-RPM-step distributions
| Step | RPM  | N (base/gale) | Baseline median (95% CI) | Gale median (95% CI) | Δ median | MW-U p |
| ...

## `handoff` cycles — per-RPM-step distributions
| ...

## `handoff` — overall (pooled across steps)
| p50 | p75 | p95 | p99 | max |

## Integrity
- algo median delta across builds: X.X%

## Asserts
- pass [baseline.samples>=expected]: ...
- pass [gale.handoff_max<=2*base_p99]: ...
```

Bootstrap CI half-widths are typically sub-cycle at N=150; the QEMU
emulated cycle counter clumps samples on integer cycles, so medians
can be exactly equal across 150 samples. Use Mann-Whitney p-values
(sensitive to distribution shape, not median width) to judge
significance.

## Assertions (what `run_qemu_bench.sh` fails on)

| Check | Rationale |
|---|---|
| samples ≥ 95% of expected | sweep must complete — audit flagged truncation |
| drops == 0 | ring buffer was sized correctly, reader kept up |
| runs ended (saw `=== END ===`) | firmware reached end-of-run deterministically |
| algo median delta < 10% | same C binary should time identically (integrity) |
| gale handoff max ≤ 2× baseline p99 | no pathological regression at the tail |

The old `handoff_mean < 800 cycles` ceiling is gone — it was a raw
threshold on a mean that couldn't be computed correctly on-target.
The new regression guard is distributional.

## Two CI lanes

| Lane | Platform | N | Samples per run | Trigger | Purpose |
|---|---|---:|---:|---|---|
| **Smoke** (`engine-bench-smoke.yml`) | QEMU `qemu_cortex_m3` | 1 | 150 | every PR | regression check, ~5 min |
| **Long** (`engine-bench-renode.yml`) | Renode `stm32f4_disco` | 1 | 7,750 | weekly + manual | authoritative numbers, ~40 min |

The smoke lane's N=1 is enough to catch integrity breaks (did it
run? are there drops? does algo agree?). The Renode long lane is
configured for `ENGINE_BENCH_SWEEP=long` — 13 RPM steps totalling
7,750 samples.

To increase statistical power on the long lane, bump `-n` in
`engine-bench-renode.yml` (each repeat is an independent boot of the
emulator).

## What the benchmark exercises

- `k_timer` (timer ISR → `crank_isr`)
- `control_step()` (pure C, in ISR context)
- `ring_buf_put` (ring-buffer with internal `k_spinlock`)
- `k_sem_give` (ISR → thread handoff)
- `k_sem_take` (thread drains sem)
- `ring_buf_get` (reader-side drain)

Only `k_sem_give` differs behaviourally between the two builds on the
bench hot path (it's the Gale FFI call site for `CONFIG_GALE_KERNEL_SEM`).
`CONFIG_GALE_KERNEL_TIMER` and `CONFIG_GALE_KERNEL_SPINLOCK` compile
in but are not directly exercised by the ISR path — kept on to
validate the module wiring.

## Knobs

`-DENGINE_BENCH_TOTAL_SAMPLES=N` overrides the compile-time total.
`-DENGINE_BENCH_SWEEP=long` selects the long RPM sweep.

If you override `TOTAL_SAMPLES`, make sure it equals
`sum(sweep[].samples)` — otherwise the reader exits mid-sweep and
steps truncate (bug fixed in #25).

## Renode note

The `renode/engine_stm32f4.robot` file uses `defaultPauseEmulation=true`
on `Create Terminal Tester`. This pauses the emulator when a
`Wait For Line On Uart` call is active; between calls the emulator
runs freely. Firmware event emission happens during the `=== END ===`
wait, so event lines are captured in the UART file backend even when
the test is paused. Renode's `CreateFileBackend` is always-on and
sees every character.

## Limitations

- QEMU cycle counter (12 MHz) is fiction; absolute ns numbers from
  the smoke lane are not realistic. Use Renode (168 MHz Cortex-M4F)
  or real hardware for citations.
- At QEMU's clock, samples collapse onto adjacent integer cycles —
  distribution tests can be overly sensitive. Rely on Mann-Whitney
  significance, not CI half-widths.
- N=1 per variant can't distinguish a real delta from run-to-run
  boot-to-boot noise. Use `-n 20` for publishable numbers; don't
  cite the `-n 1` output externally.
- Only single-CPU. SMP would need per-CPU side-channels.
