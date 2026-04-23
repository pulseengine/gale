# CTF tracing for `engine_control`

Optional: enable Zephyr CTF (Common Trace Format) tracing alongside the
bench's poor-man's CSV event stream. Both run in parallel — CSV on
UART0/console, CTF binary on UART1 — and are captured to separate
files.

See `docs/research/ctf-tracing-evaluation.md` for the full writeup on
overhead, tooling gap (babeltrace2 / `bt2` not available in the default
dev container), and when promoting from CSV to CTF makes sense.

## Files

- `../prj-ctf.conf` — Kconfig overlay, opt-in via `OVERLAY_CONFIG`.
- `qemu_cortex_m3.overlay` — DT overlay routing `zephyr,tracing-uart`
  to `&uart1`. The bench's `CMakeLists.txt` appends this automatically
  when it detects a CTF build (filename match on `OVERLAY_CONFIG`, or
  `-DENGINE_BENCH_CTF=y`).
- `parse_ctf_minimal.py` — zero-dep Python 3 decoder for CTF 1.8
  binary output. Covers kernel-object events (ISR, sem, mutex,
  timer, thread state). Fails soft on unknown event IDs.
- `run-ctf.sh` — build + run wrapper. Produces `output.csv` (UART0)
  and `channel0_0` (UART1) in the build dir.

## Quick start (baseline build, CTF overlay)

```sh
export ZEPHYR_BASE=/path/to/zephyr
export ZEPHYR_SDK_INSTALL_DIR=/path/to/zephyr-sdk-1.0.1
export GALE_ROOT=/path/to/gale

bash benches/engine_control/tracing_ctf/run-ctf.sh baseline
# => /tmp/engine-ctf/output.csv      (poor-man's CSV, as before)
# => /tmp/engine-ctf/channel0_0      (raw CTF binary)
# => /tmp/ctf-bench.histogram        (event-name counts)
# => /tmp/ctf-bench.per-isr          (events per crank ISR)
```

## Quick start (gale primitives + CTF)

```sh
bash benches/engine_control/tracing_ctf/run-ctf.sh gale
```

## Using upstream tooling (requires babeltrace2)

If `pip install bt2` + `libbabeltrace2-dev` is available on the host:

```sh
mkdir /tmp/ctf-trace
cp /tmp/engine-ctf/channel0_0 /tmp/ctf-trace/
cp $ZEPHYR_BASE/subsys/tracing/ctf/tsdl/metadata /tmp/ctf-trace/
$ZEPHYR_BASE/scripts/tracing/parse_ctf.py -t /tmp/ctf-trace
```

## Not in this prototype

- Automated overhead measurement vs CTF-off.
- Integration with `run_qemu_bench.sh` regression lane (kept separate
  on purpose — CTF overhead skews handoff cycles).
- SEGGER SystemView backend (needs real hardware with RTT).
