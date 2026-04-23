# Engine-control benchmark — adversarial methodology review

Red-team scoping pass on `benches/engine_control/` and the published numbers
(QEMU smoke -11.6% handoff mean; Renode long -6.1% / -12.5% max). Goal: find
measurement-side reasons the numbers could be misleading, independent of
whether Gale is actually faster.

## Summary

- **CRITICAL: direction of the result disagrees with the code.** Gale adds an
  out-of-line FFI call (`gale_k_sem_give_decide`) on top of the baseline
  inline branch. Net instruction count in the handoff hot-path is
  unambiguously **higher** in Gale, yet the reported handoff mean is
  **-11.6% / -6.1%** (faster). This is not explainable by the code diff; it
  is explainable by code-layout / cache / measurement-overhead asymmetries
  or by the sweep-weighting bias below. The sign of the delta is the
  finding the external claim rests on, and it does not survive first-
  principles analysis. Before citing the number externally, confirm with a
  side-by-side objdump of `crank_isr` / `z_impl_k_sem_give` and a paired
  measurement that is robust to layout.
- **Mean divisor mismatch** in `print_csv_report` — numerator counts all
  ISR events (`sum_algo` / `sum_handoff`), denominator is the reader's
  `count` (which stops at `TOTAL_SAMPLES`). Histograms in `/tmp/engine-*`
  output confirm event counts 151–152 but `count=150`; means are
  silently inflated by ~1 % on both sides. Cancels in delta only if the
  baseline-vs-Gale event-count imbalance is symmetric; it is not (152 vs
  151 in the short run).
- **Sweep doesn't reach all stations.** Short QEMU run hits `count ==
  TOTAL_SAMPLES` during step 2 of 5 — only RPM=1000 and RPM=2000 contribute
  samples. The "500→10 000 RPM sweep" label in documentation is false for
  the smoke run. Long run (7 750 scheduled samples, `TOTAL_SAMPLES=10 000`)
  *overruns* — after step 13 the timer keeps firing at RPM=10 000 until
  `count` catches up, so ~22 % of samples are at the highest RPM.
- **Single-run-per-variant.** No run-to-run variance bound. A -11.6 % delta
  on 150 samples through one QEMU execution has no statistical weight and
  cannot support an external claim.
- **Silent Kconfig drop:** `CONFIG_GALE_KERNEL_SPINLOCK_VALIDATE=y` in
  `prj-gale.conf` requires `CONFIG_SMP=y`, which is not set; the option is
  silently omitted from `autoconf.h`. Documentation claim "overlay enables
  SEM/TIMER/SPINLOCK/SPINLOCK_VALIDATE" is false in the smoke build.

## Confirmed flaws

### F1 — Mean denominator does not match sum numerator  (HIGH)

Evidence: `benches/engine_control/src/main.c:222,225`:
```c
printf("algo,mean,%llu\n",    (unsigned long long)(sum_algo / count));
printf("handoff,mean,%llu\n", (unsigned long long)(sum_handoff / count));
```
`sum_*` is incremented in every `crank_isr` invocation (main.c:168-169);
`count` is incremented by the reader thread (main.c:190) and capped at
`TOTAL_SAMPLES`. Any ISR event after the reader stops counting is included
in `sum_*` but not in `count`. Baseline CSV shows histogram totals 152 /
152 with `count=150`; Gale shows 151 / 151 with `count=150`.

Fix: track `histo_total` (or reuse `g_interrupts`) as the divisor, or stop
the timer the moment `count` reaches `TOTAL_SAMPLES` and read sums *after*.
The reporter already stops the timer in `main()` before printing; moving
the sum-reads after `k_timer_stop` and using `g_interrupts` as divisor
would close this.

### F2 — Sweep does not span the advertised RPM range  (HIGH)

Evidence: `benches/engine_control/src/main.c:183-194,234-266`. The reader
terminates the whole run at `TOTAL_SAMPLES`, not after the sweep
completes; the sweep driver never stops the timer. Observed in
`/tmp/engine-baseline/output.csv`: after "step 2/5" prints, the reporter
emits `=== END ===`, then "step 3/5: rpm=4000" arrives but never runs.
For the short sweep, 60 % of configured RPM stations contribute zero
samples. For the long sweep, `TOTAL_SAMPLES=10 000` exceeds the configured
total (7 750); the timer keeps firing at RPM=10 000 until the reader
drains 10 000 items — so the last 2 250 samples are monotonically at the
highest rate.

Fix: either (a) gate `reader_loop` on `sweep_done` rather than
`TOTAL_SAMPLES`, or (b) make `TOTAL_SAMPLES` equal to `Σ sweep[i].samples`
and have `sweep_driver` call `k_timer_stop` before the drain loop.

### F3 — Direction of the handoff delta is implausible from code alone  (HIGH)

Evidence: `zephyr/kernel/sem.c:95-121` vs `gale/zephyr/gale_sem.c:89-120`.
The Gale path adds an out-of-line `gale_k_sem_give_decide` FFI call
(`ffi/src/lib.rs:381`) with three u32 args and a struct return. Baseline
replaces the same logic with an inline `count += (count!=limit)?1:0` —
two instructions. Gale should be strictly slower. A -11.6 % or -6.1 %
"Gale faster" result must therefore come from something other than the
primitive being faster. Plausible second-order causes that are *not*
properties of Gale's implementation:

- Code-layout shift: gale_sem.c/gale_spinlock.c/gale_timer.c being linked
  as separate `zephyr_library_named` targets relocates `z_impl_k_sem_give`
  to a different flash page; hot-code alignment on Cortex-M affects
  prefetch/pipeline.
- Measurement-overhead asymmetry: `k_cycle_get_32()` on stm32f4 /
  qemu_cortex_m3 takes a `k_spinlock` (`zephyr/drivers/timer/
  cortex_m_systick.c:553-561`), which on UP reduces to PRIMASK toggle +
  a SysTick register read. Different linker layouts change the exact
  instruction sequence's alignment; the 3 reads per ISR (t_entry,
  t_algo_end, t_exit) add up.
- On QEMU, cycle counts are derived from icount-based translation —
  deterministic per build but sensitive to instruction-mix, not to
  pipeline behaviour. A layout difference that swaps a t16 Thumb
  instruction for a t32 one will shift the count noticeably.

Fix: do a `diff` of `arm-zephyr-eabi-objdump -d zephyr.elf` restricted to
`crank_isr`, `z_impl_k_sem_give`, and `sys_clock_cycle_get_32` between
baseline and gale builds. Claim a speedup only after measuring the
additional FFI-call cost in isolation and showing the layout effect is
smaller than it. A claim that "Gale is faster" based on the current data
is not defensible — at best the data shows "Gale is no slower despite the
extra FFI call".

### F4 — CONFIG_GALE_KERNEL_SPINLOCK_VALIDATE silently dropped  (MED)

Evidence: `prj-gale.conf:20` enables the option, but
`gale/zephyr/Kconfig:337-340` gates it on `depends on SMP`. The short and
long benches do not enable `CONFIG_SMP`, so the option is absent from
`/tmp/engine-gale/zephyr/include/generated/zephyr/autoconf.h`. Claims in
`prj-gale.conf` comments and in the README about what the overlay
enables are therefore false for this build.

Fix: either (a) enable `CONFIG_SMP=y` in the overlay (but then the
benchmark exercises a different scheduler path than the baseline — unfair
comparison), or (b) drop the line from `prj-gale.conf` and document that
SPINLOCK_VALIDATE is not exercised here.

### F5 — CONFIG_GALE_KERNEL_SPINLOCK and CONFIG_GALE_KERNEL_TIMER are compiled but not called  (MED)

Evidence: `grep -rn "gale_spinlock_acquire\|gale_timer_expiry"
zephyr/` returns 0 hits. The Gale shim files (`gale_spinlock.c`,
`gale_timer.c`) compile into libraries that are linked but whose
functions have no call sites in the benchmark or in Zephyr. So the
handoff delta is attributable **entirely** to `gale_k_sem_give_decide`.
The documented "Gale primitives the benchmark exercises" list in
`prj-gale.conf` overstates the scope: only `k_sem_give` (Gale) vs
`k_sem_give` (baseline) differs at runtime.

Fix: update the README and `prj-gale.conf` comments to state only the
primitive actually differing — `k_sem_give`. This also tightens the
causal chain for the delta.

### F6 — Histogram resolution inadequate for the reported delta  (MED)

Evidence: `main.c:106-114`, `bucket_of` is log2. Short run handoff
histogram:
```
bucket 7 (128-255)  baseline=78  gale=81
bucket 8 (256-511)  baseline=74  gale=70
```
A 37-cycle mean shift at mean ~300 cycles falls entirely inside bucket 8.
The histogram cannot distinguish the distributions; only the scalar mean
does. The mean is reported to 3 significant figures from 150 samples,
which is over-precision.

Fix: either compute true quantiles (p50, p95, p99) from a raw per-sample
buffer of reasonable size, or narrow histogram buckets to linear
granularity over the expected range (e.g. 16-cycle-wide buckets covering
0…1024).

### F7 — Regression threshold arithmetic is brittle  (LOW)

Evidence: `run_qemu_bench.sh:124-126`:
```sh
local diff=$(( g_algo > b_algo ? g_algo - b_algo : b_algo - g_algo ))
local pct=$(( diff * 100 / b_algo ))
```
Integer division; `diff=7`, `b_algo=79` → `pct=8` (borderline below the
10 % ceiling). A 1-cycle flap at this scale can toggle the assertion.
README and workflow comment advertise a 5 % threshold; the script uses
10. The smoke workflow at `.github/workflows/engine-bench-smoke.yml:8`
says "algo-mean matches across builds within 10%", the script at line 9
says "5%". One is wrong.

Fix: reconcile comment vs constant; use float or fixed-point to get
sub-percent resolution; refuse the run below some absolute-cycle floor
where relative % is noise.

## Ruled-out suspicions

- **Cycle counter wrap.** Total run time is ≈0.6 s of simulated wall
  clock; wrap period is 25.6 s (168 MHz) and 358 s (12 MHz). Even if the
  counter wrapped mid-interval, uint32_t subtraction handles it modulo
  2^32 correctly for any positive delta well below 2^31 cycles. Not a
  concern at the reported scales.
- **ISR concurrency / partial writes.** Cortex-M single-core, ISR is
  atomic w.r.t. the reader thread; the reporter only reads after
  `k_timer_stop`. No race.
- **ISR re-entry.** k_timer callbacks are serialised by the timeout
  subsystem; a given `crank_isr` instance runs to completion before the
  next is dispatched. Re-entry would manifest as drops, and drops=0.
- **First-sample cold-cache anomaly.** Present in principle but with
  n≥150 the first-sample contribution to the mean is <1 %. Not a
  dominant source of the reported delta.
- **Build flag leakage into algo codegen.** `compile_commands.json` for
  `main.c` / `control.c` / `tables.c` is byte-identical between builds
  except for the autoconf `-imacros` path. `autoconf.h` diff only adds
  `CONFIG_GALE_*` defines, none of which are referenced inside
  control.c/tables.c. The `#ifdef CONFIG_GALE_KERNEL_SEM` in main.c only
  picks the build-tag string literal. Algo codegen is effectively
  identical (object sizes differ by 4-8 bytes — debug-info strings).
- **ISR entry/exit frame size.** Hardware-fixed on Cortex-M; no Kconfig
  in the overlay changes EXC_RETURN semantics.

## Recommended next steps

To make the published numbers defensible:

1. Fix F1 (divisor) and F2 (sweep coverage). Re-run both smoke and long
   with the fixes; compare numbers.
2. For F3, produce a side-by-side objdump of the three hot functions and
   annotate any non-Gale-caused instruction-count differences. If Gale
   really is faster, the asm diff must explain it; if layout dominates,
   state that explicitly.
3. For F5, trim `prj-gale.conf` to `CONFIG_GALE_KERNEL_SEM=y` only, so
   the comparison measures one primitive.
4. For F6, switch to linear-bucket or sorted-buffer percentile reporting.
5. Adopt the statistical-confidence proposal below.

## Statistical-confidence proposal

Single-run deltas are not defensible. Minimum bar for the -6.1 % / -12.5 %
claim to survive external review:

- **N ≥ 30 independent runs per variant** (each firmware boot is one run).
  In Renode this means 30 fresh simulator invocations; QEMU deterministic
  output is per-seed, so include a per-run randomisation of the Zephyr
  timer initial state (`CONFIG_TIMER_RANDOM_INITIAL_STATE` already exists).
- **Primary metric: paired Mann-Whitney U test** on the per-sample
  handoff distributions (not the reported mean) from each (baseline_i,
  gale_i) pair. Report the median of medians and the 95 % CI from
  bootstrap resampling (N_bootstrap = 10 000).
- **Effect-size floor:** require |median_delta| > 1 bucket-cycle
  resolution *and* p < 0.01. Below that, publish as "no detectable
  difference" rather than a percentage.
- **Layout-robustness check:** for each variant, build under at least
  two link orderings (e.g. toggle `-ffunction-sections` off/on or
  force a benign `CONFIG_*=n` that changes object layout without
  touching semantics). If the delta sign flips across link layouts,
  the measurement is layout-dominated, not Gale-dominated.
- **Publication rule:** cite the median + 95 % CI, not the mean. Means
  on log-scale histograms with <200 samples are systematically
  misleading.

Until F1-F3 are fixed and the statistical protocol is run, the correct
external-facing statement is **"no measured regression from Gale's
k_sem_give replacement"**, not **"Gale is N % faster"**.
