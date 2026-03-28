#!/usr/bin/env bash
# size_compare.sh — Compare Zephyr ELF size with and without Gale.
#
# Usage:
#   ./scripts/size_compare.sh [TEST_PATH]
#
# Defaults to tests/kernel/semaphore/semaphore if no argument given.
#
# Prerequisites:
#   - ZEPHYR_BASE is set (or we detect it from cwd)
#   - west is available (activate .venv first)
#   - Rust toolchain with thumbv7m-none-eabi target
#
# The script builds the test twice (baseline vs Gale) and prints a
# side-by-side comparison of .text, .data, .bss, and total sizes.

set -euo pipefail

GALE_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TEST_PATH="${1:-tests/kernel/semaphore/semaphore}"
BOARD="${BOARD:-qemu_cortex_m3}"

# Resolve ZEPHYR_BASE
if [ -z "${ZEPHYR_BASE:-}" ]; then
    if [ -f "$(dirname "$GALE_ROOT")/zephyr/zephyr/VERSION" ]; then
        export ZEPHYR_BASE="$(dirname "$GALE_ROOT")/zephyr"
    else
        echo "ERROR: ZEPHYR_BASE not set. Source your Zephyr environment first."
        exit 1
    fi
fi

# Find the size tool
SIZE_TOOL=""
for candidate in \
    "$(dirname "$ZEPHYR_BASE")/zephyr-sdk-*/arm-zephyr-eabi/bin/arm-zephyr-eabi-size" \
    arm-zephyr-eabi-size \
    arm-none-eabi-size; do
    # Use bash glob expansion for the wildcard path
    for expanded in $candidate; do
        if command -v "$expanded" >/dev/null 2>&1 || [ -x "$expanded" ]; then
            SIZE_TOOL="$expanded"
            break 2
        fi
    done
done

if [ -z "$SIZE_TOOL" ]; then
    echo "ERROR: No ARM size tool found. Install Zephyr SDK or arm-none-eabi-gcc."
    exit 1
fi

echo "=== Gale Binary Size Comparison ==="
echo "Test:       $TEST_PATH"
echo "Board:      $BOARD"
echo "Gale root:  $GALE_ROOT"
echo "Zephyr:     $ZEPHYR_BASE"
echo "Size tool:  $SIZE_TOOL"
echo ""

BUILD_BASE="$(mktemp -d)"
BUILD_BASELINE="$BUILD_BASE/baseline"
BUILD_GALE="$BUILD_BASE/gale"

cleanup() {
    rm -rf "$BUILD_BASE"
}
trap cleanup EXIT

# --- Build baseline (stock Zephyr, no Gale) ---
echo ">>> Building baseline (stock Zephyr)..."
west build -b "$BOARD" \
    -s "$ZEPHYR_BASE/$TEST_PATH" \
    -d "$BUILD_BASELINE" \
    -- 2>&1 | tail -5

# --- Build with Gale ---
echo ""
echo ">>> Building with Gale overlay..."
west build -b "$BOARD" \
    -s "$ZEPHYR_BASE/$TEST_PATH" \
    -d "$BUILD_GALE" \
    -- \
    -DZEPHYR_EXTRA_MODULES="$GALE_ROOT" \
    -DOVERLAY_CONFIG="$GALE_ROOT/zephyr/gale_overlay.conf" \
    2>&1 | tail -5

echo ""
echo "=== Results ==="
echo ""

# --- Extract sizes ---
echo "--- Baseline (stock Zephyr) ---"
"$SIZE_TOOL" "$BUILD_BASELINE/zephyr/zephyr.elf"

echo ""
echo "--- With Gale ---"
"$SIZE_TOOL" "$BUILD_GALE/zephyr/zephyr.elf"

echo ""

# Parse sizes and compute delta
BASELINE_TEXT=$("$SIZE_TOOL" "$BUILD_BASELINE/zephyr/zephyr.elf" | tail -1 | awk '{print $1}')
BASELINE_DATA=$("$SIZE_TOOL" "$BUILD_BASELINE/zephyr/zephyr.elf" | tail -1 | awk '{print $2}')
BASELINE_BSS=$("$SIZE_TOOL" "$BUILD_BASELINE/zephyr/zephyr.elf" | tail -1 | awk '{print $3}')
BASELINE_TOTAL=$("$SIZE_TOOL" "$BUILD_BASELINE/zephyr/zephyr.elf" | tail -1 | awk '{print $4}')

GALE_TEXT=$("$SIZE_TOOL" "$BUILD_GALE/zephyr/zephyr.elf" | tail -1 | awk '{print $1}')
GALE_DATA=$("$SIZE_TOOL" "$BUILD_GALE/zephyr/zephyr.elf" | tail -1 | awk '{print $2}')
GALE_BSS=$("$SIZE_TOOL" "$BUILD_GALE/zephyr/zephyr.elf" | tail -1 | awk '{print $3}')
GALE_TOTAL=$("$SIZE_TOOL" "$BUILD_GALE/zephyr/zephyr.elf" | tail -1 | awk '{print $4}')

DELTA_TEXT=$((GALE_TEXT - BASELINE_TEXT))
DELTA_DATA=$((GALE_DATA - BASELINE_DATA))
DELTA_BSS=$((GALE_BSS - BASELINE_BSS))
DELTA_TOTAL=$((GALE_TOTAL - BASELINE_TOTAL))

echo "--- Delta (Gale - Baseline) ---"
printf "  .text:  %+d bytes\n" "$DELTA_TEXT"
printf "  .data:  %+d bytes\n" "$DELTA_DATA"
printf "  .bss:   %+d bytes\n" "$DELTA_BSS"
printf "  total:  %+d bytes\n" "$DELTA_TOTAL"

if [ "$DELTA_TOTAL" -gt 0 ]; then
    PCT=$(echo "scale=2; $DELTA_TOTAL * 100 / $BASELINE_TOTAL" | bc)
    printf "  (%s%% increase)\n" "$PCT"
elif [ "$DELTA_TOTAL" -lt 0 ]; then
    PCT=$(echo "scale=2; $DELTA_TOTAL * -100 / $BASELINE_TOTAL" | bc)
    printf "  (%s%% decrease)\n" "$PCT"
else
    echo "  (identical)"
fi
