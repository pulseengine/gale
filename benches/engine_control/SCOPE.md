# `engine_control` bench — scope, non-claims, and source of truth

This file is the **source of truth** for what the `engine_control`
benchmark measures, what it does not measure, and what kind of
evidence its numbers constitute. Subsequent blog posts, reports,
internal memos, and external citations import language from here.
**Do not** embed scope claims directly in published copy without
first updating this file. Inconsistency between published copy and
this file is a defect in the published copy.

## What is measured

Cycle counts on the named target at the named clock frequency, under
nominal contention from the bench harness only (no peripheral
traffic, no DMA, no inter-core activity, no production workload):

- **`algo_cycles`** — ISR-side `control_step()` execution time:
  cycle counter at ISR entry → cycle counter immediately after
  `control_step()` returns. Pure C, identical between baseline and
  gale builds; serves as the integrity check (medians must agree
  within 10%).
- **`handoff_cycles`** — ISR-side primitive cost: cycle counter
  immediately after `control_step()` → cycle counter at end of ISR.
  Covers `ring_buf_put` + `k_sem_give`. The measured engineering
  delta between baseline (stock Zephyr primitives) and gale
  (verified-Rust replacements) lives here.

Both values have **framework overhead compensation** applied: a
constant `bench_overhead_cycles` (median of 1000 empty
`k_cycle_get_32()`-pair measurements taken at boot under `irq_lock`)
is subtracted from every emitted value. The compensation constant is
emitted in the CSV header (`overhead_cycles,<value>`) and surfaced in
the analyzer's report header so any reader can audit and re-add it.
Matches Zephyr 4.4 `ztest_bench`'s `ctrl` pattern.

The current measurement target is one of:
- **Renode 1.16.0** (CI default, container-pinned), or
- **Renode nightly** (CI cycle-model A/B control), or
- **Real silicon** (when item 1 lands; STM32F4 Discovery via SWO/DWT
  capture).

The current Cortex-M target clock is **168 MHz** on `stm32f4_disco`,
**100 kHz tick** on `qemu_cortex_m3` (smoke). Numbers are not
comparable across these targets at face value because the cycle unit
differs.

## What is NOT measured

This bench produces engineering measurements; it is **not**
certification evidence and does **not** measure any of the following:

- **Peripheral contention** — no SPI, I²C, UART RX, GPIO toggle, or
  bus-master traffic during the measurement window. The ISR is
  driven by an internal `k_timer`, not by an external sensor.
- **DMA-driven I/O** — real flight controllers receive sensor data
  via DMA-complete IRQs with bursty alignment characteristics
  (cache, bus arbitration). This bench uses a synthetic timer ISR
  with no DMA path.
- **SMP / multi-core** — single-CPU only. The `gale_spinlock`
  primitive ships in the codebase but its actual hazard
  (concurrent CAS from another core) is **not exercised by this
  bench**. SMP coverage is a separate workflow (`zephyr-smp-test`
  on `qemu_x86_64`) with known runtime issues.
- **WCET (Worst-Case Execution Time)** — the bench reports observed
  cycle distributions. It does **not** prove a worst-case bound.
  Establishing WCET requires static analysis tooling such as
  **AbsInt aiT**, **Rapita RapiTime**, or **OTAWA** combined with
  microarchitectural models for the specific MCU. Worst-case-observed
  numbers, when added later under the bench-rigor work item 6, are
  **not** WCET claims and must be labeled as `worst_observed`,
  not `wcet`. The distinction is unambiguous and not negotiable in
  published copy: an observation is not a proof.
- **Power consumption** — the bench measures cycles, not energy or
  current. For embedded deployment the relevant figure is often
  µJ/op or mA average, neither of which this bench produces.
- **Memory pressure** — peak heap, peak stack high-water mark, slab
  fragmentation. Stack high-water-mark capture is planned (work
  item 5, gated on real-silicon anchor first).
- **Fault tolerance** — stuck-sensor inputs, dropped messages,
  scheduler-induced timeouts, watchdog resets. The bench operates
  under **nominal** scheduling only. Fault-injection coverage is
  out of scope here and belongs in a v2 of the flight bench.
- **Long-duration drift** — runs are seconds to minutes, not hours.
  32-bit cycle-counter wrap behavior, accumulated heap fragmentation,
  ring-buffer head/tail drift over multi-hour operation are not
  observable in this bench.

## Status of the published delta

The headline `−34.5%` handoff-cycle delta (gale vs GCC baseline) is:

- **Real** — the cross-Renode A/B (1.16.0 vs nightly) shows 0.0%
  drift on identical ELFs across simulator versions, ruling out the
  cycle model as the source. The synth-vs-rustc-direct cross-check
  shows synth's codegen agrees with (in fact slightly outperforms)
  rustc-direct, ruling out a synth miscompile.
- **Tool-bounded** — produced by the on-target `k_cycle_get_32`
  reading inside Renode's per-block cost simulation. **Not** anchored
  to a real silicon measurement until work item 1 lands.
- **Workload-bounded** — measured in the engine_control ISR shape
  (one timer ISR, one ring + sem hop). **Not** generalizable to
  composed workloads (use `flight_control` for that, with its own
  scope file).

## What kind of evidence this is

**Engineering measurement** under controlled simulation, with the
methodology and toolchain enumerated in the build manifest. Suitable
for:

- Internal regression detection (CI-gated p99 ≤ 2× baseline asserts)
- Engineering decisions about primitive choice
- Public claims of the form "we measure X cycles under conditions Y"
  with conditions Y enumerated above

**Not** suitable for:

- Certification submissions to DO-178C, ISO 26262, IEC 61508, or any
  other safety standard. Certification evidence requires qualified
  tools, independent verification, requirements traceability, and
  WCET via static analysis — none of which this bench provides.
- Marketing copy that elides the conditions
- Citation as "verified-for-flight" performance

Short version for first paragraphs of any blog post: *"Cycle
measurements under Renode-simulated Cortex-M4F at 168 MHz on a
synthetic ISR workload. Engineering measurement, not certification
evidence; see SCOPE.md for the full enumeration of what is and isn't
measured."*

## When to update this file

Whenever:

- The measurement target changes (e.g., real silicon arrives — work
  item 1).
- The compensation regime changes (e.g., overhead compensation lands
  — work item 2; algorithm or constants change later).
- The non-claims list changes (e.g., SMP coverage is added; fault
  injection is added in a v2).
- A reviewer raises a scope question that the current text does not
  unambiguously answer.

Pre-compensation and post-compensation numbers are **different
measurements**. When the compensation regime changes, anchor
explicitly in published copy: *"Numbers below are
overhead-compensated; pre-compensation reference values are at
[link]"* — never combine them in the same comparison table.
