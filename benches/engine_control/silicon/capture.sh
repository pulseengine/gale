#!/usr/bin/env bash
# Silicon-anchor capture wrapper for engine_control.
#
# Builds, flashes, and captures one variant on a real board, then
# writes the result + manifest into a dated directory under runs/.
# Manual flow — not invoked from CI.
#
# Usage:
#   capture.sh --board nucleo_g474re --variant {baseline,gale} \
#              [--tick-source {systick,lptim}] \
#              [--sweep {short,long}] [--port /dev/cu.usbmodem11403]
#
# Defaults:
#   --tick-source systick  (Cortex-M default; lptim selects the STM32
#                          LPTIM-based kernel tick — see board README
#                          for the clock-source caveat)
#   --sweep short  (use --sweep long for the publication-grade run)
#   --port: auto-detect first /dev/cu.usbmodem* (macOS) or
#           /dev/ttyACM0 (Linux). Override if multiple boards present.
#
# A publication-grade anchor on a given board is the 4-run matrix:
#   variant ∈ {baseline, gale}  ×  tick_source ∈ {systick, lptim}.

set -euo pipefail

# --------------------------------------------------------------------- args
BOARD=""
VARIANT=""
SWEEP="short"
PORT=""
TICK_SOURCE="systick"
SILICON_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GALE_ROOT="$(cd "${SILICON_DIR}/../../.." && pwd)"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --board)        BOARD="$2"; shift 2 ;;
    --variant)      VARIANT="$2"; shift 2 ;;
    --sweep)        SWEEP="$2"; shift 2 ;;
    --port)         PORT="$2"; shift 2 ;;
    --tick-source)  TICK_SOURCE="$2"; shift 2 ;;
    -h|--help)
      awk '/^set -/{exit} NR>1{sub(/^# ?/, ""); print}' "$0"; exit 0 ;;
    *)
      echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

[[ -z "$BOARD"   ]] && { echo "missing --board" >&2; exit 2; }
[[ -z "$VARIANT" ]] && { echo "missing --variant" >&2; exit 2; }
case "$VARIANT" in baseline|gale) ;; *)
  echo "--variant must be 'baseline' or 'gale'" >&2; exit 2 ;;
esac
case "$SWEEP" in short|long) ;; *)
  echo "--sweep must be 'short' or 'long'" >&2; exit 2 ;;
esac
case "$TICK_SOURCE" in systick|lptim) ;; *)
  echo "--tick-source must be 'systick' or 'lptim'" >&2; exit 2 ;;
esac

# Verify board overlay exists in our silicon/boards/ tree
BOARD_DIR="${SILICON_DIR}/boards/${BOARD}"
if [[ ! -d "$BOARD_DIR" ]]; then
  echo "no silicon overlay for board '$BOARD' at $BOARD_DIR" >&2
  echo "supported: $(ls "${SILICON_DIR}/boards/" 2>/dev/null | tr '\n' ' ')" >&2
  exit 2
fi

# --------------------------------------------------------------------- env
: "${ZEPHYR_BASE:?need ZEPHYR_BASE in env}"
: "${ZEPHYR_SDK_INSTALL_DIR:=}"  # optional; west picks one up if unset
GALE_SHA_FULL="$(git -C "$GALE_ROOT" rev-parse HEAD)"
GALE_SHA="${GALE_SHA_FULL:0:8}"
DATE="$(date -u +%Y-%m-%d)"
RUNS_DIR_BASE="${SILICON_DIR}/runs"
RUN_DIR="${RUNS_DIR_BASE}/${DATE}-${BOARD}-${GALE_SHA}-${VARIANT}-${TICK_SOURCE}"
BUILD_DIR="/tmp/silicon-${BOARD}-${VARIANT}-${TICK_SOURCE}"

if [[ -d "$RUN_DIR" ]]; then
  echo "ERROR: run dir already exists: $RUN_DIR" >&2
  echo "Per protocol, never overwrite. Start a new dated dir or delete the old one." >&2
  exit 3
fi

# --------------------------------------------------------------------- port autodetect
if [[ -z "$PORT" ]]; then
  case "$(uname -s)" in
    Darwin)
      PORT="$(ls /dev/cu.usbmodem* 2>/dev/null | head -1 || true)" ;;
    Linux)
      PORT="$(ls /dev/ttyACM* 2>/dev/null | head -1 || true)" ;;
  esac
  [[ -z "$PORT" ]] && {
    echo "could not auto-detect serial port; pass --port" >&2; exit 2;
  }
  echo "auto-detected port: $PORT"
fi

# --------------------------------------------------------------------- build
echo "==> Building $VARIANT for $BOARD (sweep=$SWEEP, tick=$TICK_SOURCE)"
WEST_ARGS=( -b "$BOARD" -d "$BUILD_DIR" -s "${GALE_ROOT}/benches/engine_control" )
WEST_DEFINES=( -DENGINE_BENCH_SWEEP="$SWEEP" )

# Compose OVERLAY_CONFIG from up to three layers, in deterministic order:
#   1. gale primitive overlay      (only when --variant gale)
#   2. board silicon-overlay       (board-specific defaults)
#   3. tick-source overlay         (only when not the board's default tick)
# Zephyr semantics: later overlays override earlier ones.
OVERLAYS=()
[[ "$VARIANT" == "gale" ]] && OVERLAYS+=("${GALE_ROOT}/benches/engine_control/prj-gale.conf")

BOARD_OVERLAY="${BOARD_DIR}/prj.conf"
[[ -s "$BOARD_OVERLAY" ]] && OVERLAYS+=("$BOARD_OVERLAY")

# Tick-source overlay: only layered when the user picked a non-default
# tick. SysTick is the Cortex-M default so its overlay (if any) is opt-in.
TICK_OVERLAY="${BOARD_DIR}/prj-tick-${TICK_SOURCE}.conf"
if [[ "$TICK_SOURCE" != "systick" ]]; then
  if [[ ! -s "$TICK_OVERLAY" ]]; then
    echo "no tick-source overlay for '$TICK_SOURCE' at $TICK_OVERLAY" >&2
    exit 2
  fi
  OVERLAYS+=("$TICK_OVERLAY")
fi

[[ "$VARIANT" == "gale" ]] && WEST_DEFINES+=( -DZEPHYR_EXTRA_MODULES="$GALE_ROOT" )
if [[ ${#OVERLAYS[@]} -gt 0 ]]; then
  IFS=';' WEST_DEFINES+=( -DOVERLAY_CONFIG="${OVERLAYS[*]}" )
  unset IFS
fi

rm -rf "$BUILD_DIR"
( cd "$GALE_ROOT/.." && west build -p auto "${WEST_ARGS[@]}" -- "${WEST_DEFINES[@]}" )

ELF="${BUILD_DIR}/zephyr/zephyr.elf"
[[ ! -f "$ELF" ]] && { echo "build did not produce $ELF" >&2; exit 4; }

# --------------------------------------------------------------------- record
mkdir -p "$RUN_DIR"
cp "$ELF" "$RUN_DIR/firmware.elf"

if command -v sha256sum >/dev/null 2>&1; then
  ELF_SHA="$(sha256sum "$ELF" | awk '{print $1}')"
else
  ELF_SHA="$(shasum -a 256 "$ELF" | awk '{print $1}')"  # macOS fallback
fi
echo "$ELF_SHA  firmware.elf" > "$RUN_DIR/firmware.elf.sha256"

# --------------------------------------------------------------------- flash
echo "==> Flashing"
( cd "$GALE_ROOT/.." && west flash -d "$BUILD_DIR" )

# --------------------------------------------------------------------- capture
# Long sweep can take a few minutes wall-time at 168 MHz; short ~10s.
TIMEOUT=1800   # 30 min
[[ "$SWEEP" == "short" ]] && TIMEOUT=120

echo "==> Capturing from $PORT (timeout ${TIMEOUT}s)"
python3 "${SILICON_DIR}/capture.py" \
  --port "$PORT" --baud 115200 \
  --sentinel "=== END ===" \
  --timeout "$TIMEOUT" \
  --out "$RUN_DIR/output.csv"

# --------------------------------------------------------------------- tag
RUN_ID="silicon-${DATE}"   # deterministic per-day-per-board; tag_events
                           # prefixes with R, so this becomes R-silicon-...
python3 "${GALE_ROOT}/benches/engine_control/tag_events.py" \
  "$RUN_DIR/output.csv" "$RUN_ID" "$VARIANT" \
  > "$RUN_DIR/events.csv"

# --------------------------------------------------------------------- manifest
MANIFEST="$RUN_DIR/manifest.txt"
{
  echo "# Silicon-anchor manifest"
  echo "# Produced by benches/engine_control/silicon/capture.sh"
  echo "captured_at:           $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "board:                 ${BOARD}"
  echo "variant:               ${VARIANT}"
  echo "tick_source:           ${TICK_SOURCE}"
  echo "sweep:                 ${SWEEP}"
  echo "gale_sha:              ${GALE_SHA_FULL}"
  echo "gale_status:           $(cd "$GALE_ROOT" && git status --porcelain | wc -l | tr -d ' ') uncommitted file(s)"
  echo "host:                  $(uname -srm)"
  echo "rustc:                 $(rustc --version 2>&1 | head -1)"
  echo "cargo:                 $(cargo --version 2>&1 | head -1)"
  echo "west:                  $(west --version 2>&1 | head -1)"
  echo "zephyr_base:           ${ZEPHYR_BASE}"
  echo "zephyr_sha:            $(git -C "$ZEPHYR_BASE" rev-parse HEAD 2>/dev/null || echo unknown)"
  echo "sdk_dir:               ${ZEPHYR_SDK_INSTALL_DIR:-auto-detected by west}"
  echo "elf_sha256:            ${ELF_SHA}"
  echo "csv_sha256:            $({ sha256sum "$RUN_DIR/output.csv" 2>/dev/null \
                                  || shasum -a 256 "$RUN_DIR/output.csv"; } | awk '{print $1}')"
  echo "csv_bytes:             $(wc -c < "$RUN_DIR/output.csv" | tr -d ' ')"
  echo "csv_event_lines:       $(grep -c '^E,' "$RUN_DIR/output.csv" || echo 0)"
  echo "serial_port:           ${PORT}"
  echo "capture_timeout_s:     ${TIMEOUT}"
} > "$MANIFEST"

# --------------------------------------------------------------------- summary
echo
echo "=========================================================="
echo " Silicon capture complete"
echo "  board:        $BOARD"
echo "  variant:      $VARIANT"
echo "  tick_source:  $TICK_SOURCE"
echo "  sweep:        $SWEEP"
echo "  events:       $(grep -c '^E,' "$RUN_DIR/output.csv" || echo 0)"
echo "  manifest:     $MANIFEST"
echo "  events.csv:   $RUN_DIR/events.csv"
echo "=========================================================="
echo
echo "Next steps:"
echo "  1) sanity-check the output: head -20 $RUN_DIR/output.csv"
echo "  2) commit the run dir:"
echo "       git add benches/engine_control/silicon/runs/${DATE}-${BOARD}-${GALE_SHA}-${VARIANT}-${TICK_SOURCE}"
echo "  3) (after all 4 variant×tick_source runs captured) compare against"
echo "     the matching Renode CI:"
echo "       python3 benches/engine_control/analyze.py \\"
echo "         --baseline silicon/runs/${DATE}-${BOARD}-${GALE_SHA}-baseline-${TICK_SOURCE}/events.csv \\"
echo "         --gale     silicon/runs/${DATE}-${BOARD}-${GALE_SHA}-gale-${TICK_SOURCE}/events.csv"
