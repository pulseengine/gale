# Evaluation: Full Zephyr CTF tracing vs. poor-man's CSV for `engine_control`

**Status:** prototype files committed; build + run verification pending
(dev-container blocks `west build` from this agent's sandbox). The
tradeoff analysis below is based on code reading + synthetic-payload
testing of the decoder, not on a live QEMU capture.

**Audience:** engineers deciding whether to replace the poor-man's
`E,seq,step,rpm,algo,handoff` CSV event stream with Zephyr's Common
Trace Format (CTF 1.8) output. Both streams are complementary, not
mutually exclusive — the prototype runs them side-by-side.

## 1. What was built

| File | Role |
|---|---|
| `benches/engine_control/prj-ctf.conf` | Overlay: `CONFIG_TRACING=y + CONFIG_TRACING_CTF=y + CONFIG_TRACING_BACKEND_UART=y + CONFIG_TRACING_ASYNC=y`. Composable with `prj-gale.conf`. |
| `benches/engine_control/tracing_ctf/qemu_cortex_m3.overlay` | DT overlay routing `zephyr,tracing-uart` to `&uart1` (uart0 stays the CSV console). Deletes the now-conflicting `zephyr,uart-pipe = &uart1` chosen, which the bench never uses. Applied only when CTF is enabled (see CMakeLists gate). |
| `benches/engine_control/CMakeLists.txt` (edit) | When `OVERLAY_CONFIG` contains `prj-ctf` (or `-DENGINE_BENCH_CTF=y` is passed), adds `-serial file:channel0_0` to `QEMU_EXTRA_FLAGS` on `qemu_*` boards and appends the tracing-uart DT overlay. Default baseline / gale CI runs are unaffected. |
| `benches/engine_control/tracing_ctf/parse_ctf_minimal.py` | ~180 LOC Python decoder for CTF 1.8 binary. Covers every kernel event ID Gale + Zephyr kernel emit in the ISR path (ISR, sem, mutex, timer, thread switch, thread state). No external deps — no `bt2`, no babeltrace2. Histogram + per-ISR grouping modes. |
| `benches/engine_control/tracing_ctf/run-ctf.sh` | Build + run wrapper. Writes `output.csv` (UART0) and `channel0_0` (UART1), then runs the parser. |

The existing bench source (`src/main.c`) is **not modified**. The
sibling agent's raw-event-stream refactor (#25) is orthogonal: CSV on
UART0, CTF on UART1, both generated in the same run.

## 2. What works (code-reading / decoder-only confidence)

- **Gale's shims already emit CTF.** `gale_sem.c` and `gale_condvar.c`
  contain 7 and 8 `SYS_PORT_TRACING_OBJ_FUNC_*` invocations respectively.
  These expand to the same `sys_trace_*` calls the stock Zephyr
  primitives use, so enabling CTF "just works" once the backend is
  configured — no Gale code change needed.
- **Decoder verified against a synthetic stream.** A 32-byte synthetic
  `isr_enter → sem_give_enter → sem_give_exit → isr_exit` sequence
  decodes correctly, including `--per-isr` grouping. Event IDs 0x10-0x35
  and 0x7F-0x80 are schema-covered.
- **Bench source is untouched**, eliminating collision risk with the
  concurrent raw-event-stream refactor.

## 3. What is pending verification

The `west build -b qemu_cortex_m3 …` invocation required to actually
build + run the CTF variant is blocked in this agent's sandbox. The
following items are therefore **unmeasured**:

- Whether `CONFIG_TRACING_BACKEND_UART` links cleanly on qemu_cortex_m3
  (uart1 is normally the `zephyr,uart-pipe` chosen; we override it).
- Whether QEMU's `-serial none` default (vs. our `-serial file:…`)
  routes uart1 correctly. Upstream `samples/subsys/tracing` proves this
  pattern works on `mps2/an521` and `qemu_x86`; lm3s6965 with three
  UARTs is a strictly easier case, but unverified here.
- Overhead in cycles: expected ~200-600 cycles per event based on
  async backend + ring-buffer copy + irq_lock/unlock. With ~5-10 events
  per `crank_isr` invocation (ISR enter/exit + sem_give enter/exit +
  async worker wakeup), that's ~1000-6000 cycles added to the handoff
  measurement. At QEMU's 12 MHz counter this is ~80-500 μs, which
  **exceeds the ~320 baseline cycles of handoff itself** — CTF would
  dominate what we're trying to measure.
- Whether `CONFIG_TRACING_ASYNC`'s tracing thread (priority 6,
  below the reader at 5) meaningfully decouples emission latency from
  ISR latency, or whether the ring buffer fills faster than the worker
  drains it at 60 kHz interrupt rates.

## 4. Tooling gap: `bt2` is not available

Upstream's `scripts/tracing/parse_ctf.py` requires babeltrace2's Python
bindings (`import bt2`). Neither the Zephyr SDK (1.0.1) nor the dev
container ships them, and `pip install` is blocked by sandbox policy.
The minimal parser in `tracing_ctf/parse_ctf_minimal.py` avoids the
dependency at the cost of only supporting fixed-layout events — unknown
event IDs cannot be length-decoded and will desync the stream.

For operator-side analysis with the full upstream tool, run
`pip install --user babeltrace2` on a host with libbabeltrace2 (e.g.
`brew install babeltrace`) and point `scripts/tracing/parse_ctf.py -t`
at a directory containing the captured `channel0_0` and the metadata
file copied from `zephyr/subsys/tracing/ctf/tsdl/metadata`.

## 5. Recommendation

**Keep the poor-man's CSV as the default bench output.** Reasons:

1. **Measurement fidelity.** CTF emission overhead (~1-6k cycles/ISR)
   is the same order of magnitude as the handoff we're measuring
   (~200-400 cycles). Even async backend copies are in the ring
   buffer's hot path and add irq_lock scope. Running CTF and the CSV
   stream simultaneously means the CSV now measures "handoff + CTF
   overhead", not handoff.
2. **Toolchain friction.** Full CTF decode needs babeltrace2 + `bt2`.
   That's a separate install we'd either bundle in CI (adds ~60 MB)
   or document as an operator burden. The minimal parser covers the
   kernel events we care about but not the long tail (GPIO, net,
   syscall, etc.).
3. **Signal-to-noise.** The bench asks one question: "does Gale's
   sem_give path latency match or beat stock?" A six-field CSV line
   answers that directly. CTF gives us the entire kernel's behaviour,
   ~95% of which is noise for this benchmark.

**Promote to CTF when:**

- We need to **attribute** a handoff-latency spike to a specific
  kernel call (is it `k_sem_give` itself? the scheduler wake?
  the ring_buf spinlock? the IRQ return path?). CSV cannot tell
  these apart; CTF can.
- We need **cross-thread causality**. "ISR wakes reader, reader
  preempts sweep thread" is trivial in CTF, impossible in CSV.
- We add **multi-primitive benches** (e.g. mutex + condvar + event
  flags in a producer-consumer chain). The poor-man's approach
  doesn't scale to >1 latency measurement per interrupt without
  ballooning the CSV schema.
- We port to hardware with a **dedicated trace pin / SWO** (STM32F4,
  nRF52). Then CTF emission has its own channel and doesn't compete
  with the bench's measurement instruments.

**Interim:** ship the CTF prototype (this commit) as an optional
`OVERLAY_CONFIG=prj-ctf.conf` recipe. Anyone debugging a handoff
regression can turn it on, see the kernel call graph, turn it off for
the regression-check run. Don't wire it into `run_qemu_bench.sh`.

## 6. Next steps (for follow-up)

- [ ] Actually run `tracing_ctf/run-ctf.sh` end-to-end in a sandbox
      that allows `west build`. Capture `channel0_0`, feed to the
      minimal parser, confirm ISR + sem_give pairs appear.
- [ ] Measure the CTF-on vs CTF-off `handoff` delta across 1000 samples
      at RPM 6000 (hot path). Use the CSV output (which is unchanged
      in structure) — compare mean, p99, max.
- [ ] If the CSV numbers blow up >2× with CTF on, confirm the
      measurement dominance claim in §3 and set a hard requirement:
      CTF and CSV cannot be captured in the same run for the
      regression benchmark.
- [ ] Check whether `CONFIG_TRACING_ASYNC=n` (sync backend) makes the
      overhead worse or better — sync avoids the worker thread but
      blocks on every uart_poll_out.
- [ ] Decide whether to package `babeltrace2` in the dev container.
