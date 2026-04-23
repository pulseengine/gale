#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Build and run the engine_control bench with Zephyr CTF tracing
# enabled. Writes:
#   <build>/output.csv       — poor-man's CSV event stream (UART0/console)
#   <build>/channel0_0       — raw CTF binary stream (UART1, tracing backend)
#
# Then dumps a histogram of CTF events via parse_ctf_minimal.py to
# /tmp/ctf-bench.histogram.
#
# This does NOT replace run_qemu_bench.sh; it's a parallel evaluation
# path to measure CTF overhead and decoder feasibility. See
# docs/research/ctf-tracing-evaluation.md for interpretation.

set -euo pipefail

: "${ZEPHYR_WORKSPACE:=/Users/r/git/pulseengine/z}"
: "${ZEPHYR_BASE:=${ZEPHYR_WORKSPACE}/zephyr}"
: "${ZEPHYR_SDK_INSTALL_DIR:=/Users/r/zephyr-sdk-1.0.1}"
: "${GALE_ROOT:=${ZEPHYR_WORKSPACE}/gale}"
: "${BUILD_ROOT:=/tmp}"
: "${BOARD:=qemu_cortex_m3}"

export ZEPHYR_BASE ZEPHYR_SDK_INSTALL_DIR

BENCH="${GALE_ROOT}/benches/engine_control"
BUILD="${BUILD_ROOT}/engine-ctf"
OVERLAY="${BENCH}/prj-ctf.conf"
VARIANT="${1:-baseline}"  # baseline | gale

extra_overlays=""
if [ "${VARIANT}" = "gale" ]; then
	extra_overlays="${BENCH}/prj-gale.conf;"
fi

rm -rf "${BUILD}"
west build -b "${BOARD}" -d "${BUILD}" -s "${BENCH}" \
	-- -DOVERLAY_CONFIG="${extra_overlays}${OVERLAY}" \
	>"${BUILD_ROOT}/engine-ctf.build.log" 2>&1 \
	|| { echo "build failed — see ${BUILD_ROOT}/engine-ctf.build.log"; tail -40 "${BUILD_ROOT}/engine-ctf.build.log"; exit 1; }

echo "==> built: ${BUILD}/zephyr/zephyr.elf ($(stat -f%z "${BUILD}/zephyr/zephyr.elf" 2>/dev/null || stat -c%s "${BUILD}/zephyr/zephyr.elf") bytes)"

# run QEMU. Our CMakeLists appended `-serial file:channel0_0` as the
# second -serial (uart1 → tracing). stdout is the console (UART0 / CSV).
(cd "${BUILD}" && rm -f channel0_0 && timeout 90 west build -t run 2>&1) \
	| tee "${BUILD}/output.csv"

echo ""
echo "==> CSV (UART0):   ${BUILD}/output.csv"
echo "==> CTF (UART1):   ${BUILD}/channel0_0"

if [ ! -s "${BUILD}/channel0_0" ]; then
	echo "!!! channel0_0 is empty or missing — QEMU did not route UART1 output."
	echo "!!! check CMakeLists.txt QEMU_EXTRA_FLAGS and board overlay."
	exit 2
fi

echo ""
echo "==> CTF histogram:"
python3 "${BENCH}/tracing_ctf/parse_ctf_minimal.py" \
	--histogram "${BUILD}/channel0_0" | tee "${BUILD_ROOT}/ctf-bench.histogram"

echo ""
echo "==> per-ISR breakdown:"
python3 "${BENCH}/tracing_ctf/parse_ctf_minimal.py" \
	--per-isr "${BUILD}/channel0_0" | tee "${BUILD_ROOT}/ctf-bench.per-isr"
