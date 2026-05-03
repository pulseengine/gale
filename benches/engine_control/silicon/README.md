# Silicon-anchor protocol — engine_control

CI runs Renode (deterministic, parallel-safe). **Silicon runs are
manual**, periodic, and hand-driven on a single shared board.
This directory contains the protocol for taking a silicon capture,
recording it as immutable evidence in the repo, and citing it as
the anchor for Renode-headlined published numbers.

## Why

Renode is per-translated-block instruction-cost simulation, not
microarchitectural simulation: no cache, no memory contention, no
pipeline modeling. The cross-Renode A/B (1.16.0 vs nightly = 0.0%
drift) ruled out simulator-version drift but did NOT rule out
Renode being systematically off vs real silicon by a fixed
multiplier. The silicon anchor settles that.

The relationship `silicon_cycles / renode_cycles = R` is what the
silicon anchor establishes. Once `R` is consistent across
multiple silicon captures over time, it can be cited as the
Renode-silicon multiplier for that bench/board combination.

## Recorded-run-in-git protocol

Every silicon run lives in `silicon/runs/<YYYY-MM-DD>-<board>-<gale-sha>-<variant>/`
and contains:

- `output.csv` — the raw UART capture (firmware-emitted)
- `events.csv` — same data, tagged through `tag_events.py`
- `manifest.txt` — board, MCU, clock, rustc/cargo versions, gale
  commit SHA, ELF sha256, capture timestamp
- `firmware.elf` — the exact binary that produced the capture
- `firmware.elf.sha256` — checksum file

These directories are **immutable** once committed. To re-run the
same capture, create a new dated directory; never overwrite an
existing one. This makes any silicon citation in a blog post or
report point to a stable git URL.

CSV row counts are small (~50–500 KB per run, ~7,750 rows long
sweep). At one capture per board per major bench-relevant commit,
the repo growth is modest.

## Boards

| Board | Status | Anchors |
|---|---|---|
| `nucleo_g474re` (STM32G474RE, Cortex-M4F, 170 MHz) | scaffold ready | the existing Renode `stm32f4_disco` Cortex-M numbers |
| `esp32c3_devkit_rust1` (ESP32-C3, RV32IMC, 160 MHz) | not started | the *future* RISC-V Renode lane (separate work) |

## Capture procedure (NUCLEO-G474RE)

Hardware:
- Hardware: STMicroelectronics NUCLEO-G474RE
- Connection: USB to host (ST-Link integrated, virtual COM port at 115200 8N1)
- Programming: `west flash` via OpenOCD or pyOCD (ST-Link backend)

Host setup (one-time):
- Zephyr SDK with `arm-zephyr-eabi` toolchain
- OpenOCD or pyOCD installed (`brew install open-ocd` on macOS, or `apt install openocd`)
- Python with `pyserial` for the capture script: `pip3 install pyserial`

To take a baseline capture (stock Zephyr):

```sh
cd $GALE_ROOT
bash benches/engine_control/silicon/capture.sh \
    --board nucleo_g474re \
    --variant baseline \
    --sweep long
```

To take a gale capture:

```sh
bash benches/engine_control/silicon/capture.sh \
    --board nucleo_g474re \
    --variant gale \
    --sweep long
```

Both invocations:

1. Build the firmware locally (no Bazel; `west build -b <board>`).
2. Compute the firmware ELF sha256.
3. Flash via `west flash`.
4. Open the board's USB CDC serial port and read until `=== END ===`
   (default timeout: 30 minutes for `--sweep long`).
5. Generate `manifest.txt` from the build environment + capture
   metadata.
6. Tag the raw output through `tag_events.py` (run-id auto-derived
   from the date + board).
7. Write everything into a new `silicon/runs/<dir>/`.

The capture script does not commit. After both variants are
captured and you've eyeballed `output.csv` for sanity, commit:

```sh
git add benches/engine_control/silicon/runs/<YYYY-MM-DD>-nucleo_g474re-*-{baseline,gale}/
git commit -m "silicon: NUCLEO-G474RE anchor at gale@<short-sha>"
```

## Comparing silicon vs Renode

Once `silicon/runs/<dated-dir>-{baseline,gale}/` exist, run:

```sh
python3 benches/engine_control/analyze.py \
    --baseline silicon/runs/<dir-baseline>/events.csv \
    --gale     silicon/runs/<dir-gale>/events.csv \
    --runs 1 \
    > /tmp/silicon-comparison.md
```

The analyzer renders the same baseline-vs-gale tables as for
Renode, but the metadata in the report header carries through the
silicon-run identifiers. Compare side-by-side with the Renode CI
output for the same gale SHA — the **ratio** `silicon_median /
renode_median` per RPM step is the calibration data.

If you want a single-call Renode-vs-silicon side-by-side rendering,
that's a planned analyzer extension (`--silicon-anchor <events.csv>`)
to be added once the first capture exists to test against.

## Anchor cadence

- One silicon capture per board per major bench-relevant gale
  commit (e.g., when overhead compensation lands, when synth
  pipeline changes, when a primitive's hot-path is rewritten).
- Each Renode-headlined publication cites the most recent matching
  anchor by stable git URL.
- Three to four anchor points per board per year is enough to
  claim the Renode-silicon relationship is monotonic.

## Don't

- Don't overwrite an existing `runs/<dated-dir>/` — start a new one.
- Don't combine pre-overhead-compensation and post-overhead-
  compensation captures in the same comparison table; they're
  different measurements (see `../SCOPE.md`).
- Don't claim WCET from silicon captures. Worst-case-observed is
  not WCET. Same rule as the synthetic bench (see `../SCOPE.md`).
- Don't run silicon captures from a branch that isn't reproducible
  (uncommitted changes). The manifest captures the working-tree
  state, not just HEAD.
