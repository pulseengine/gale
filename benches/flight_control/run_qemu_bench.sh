#!/usr/bin/env bash
# Flight-control macro-benchmark — QEMU event-stream runner.
#
# Mirrors benches/engine_control/run_qemu_bench.sh — see that file for
# methodology rationale (post-#25 raw event stream, off-target stats).
# This script:
#   1. Builds baseline + gale variants of the macro bench
#   2. Runs each N times in qemu_cortex_m3
#   3. Tags raw output with a run-id, concatenates into events.csv
#   4. Invokes analyze.py with the per-step + Mann-Whitney logic
#      extended for the four new cycle-delta columns (t_lock, t_post,
#      t_round, t_bcast).
#
# Usage:
#   ./run_qemu_bench.sh [-n RUNS]

set -euo pipefail

: "${ZEPHYR_WORKSPACE:=/Users/r/git/pulseengine/z}"
: "${ZEPHYR_BASE:=${ZEPHYR_WORKSPACE}/zephyr}"
: "${ZEPHYR_SDK_INSTALL_DIR:=/Users/r/zephyr-sdk-1.0.1}"
: "${GALE_ROOT:=${ZEPHYR_WORKSPACE}/gale}"
: "${BUILD_ROOT:=/tmp}"

export ZEPHYR_BASE ZEPHYR_SDK_INSTALL_DIR

RUNS=1
while [ "$#" -gt 0 ]; do
	case "$1" in
	-n) RUNS="$2"; shift 2;;
	-h|--help)
		echo "Usage: $0 [-n RUNS]"
		exit 0;;
	*)
		echo "unknown arg: $1" >&2
		exit 2;;
	esac
done

# Per-run QEMU timeout. Macro bench has a heavier control loop so the
# 150-sample short sweep takes ~30-45 s in QEMU; long sweep (4500
# samples) takes much longer and is meant for Renode. 90 s covers the
# short variant comfortably.
readonly PER_RUN_TIMEOUT=90

build() {
	local name="$1"
	local dir="${BUILD_ROOT}/flight-${name}"
	local extra=()
	if [ "${name}" = "gale" ]; then
		extra+=( -DZEPHYR_EXTRA_MODULES="${GALE_ROOT}" )
		extra+=( -DOVERLAY_CONFIG="${GALE_ROOT}/benches/flight_control/prj-gale.conf" )
	fi
	rm -rf "${dir}"
	west build -b qemu_cortex_m3 -d "${dir}" \
	    -s "${GALE_ROOT}/benches/flight_control" \
	    ${extra[@]+"--"} ${extra[@]+"${extra[@]}"} \
	  >"${dir}.build.log" 2>&1
	echo "==> built ${name}: ${dir}/zephyr/zephyr.elf"
}

run_one() {
	local name="$1" run_id="$2"
	local dir="${BUILD_ROOT}/flight-${name}"
	local raw="${dir}/run${run_id}.raw"
	local events="${dir}/events.csv"
	timeout "${PER_RUN_TIMEOUT}" west build -d "${dir}" -t run \
	    >"${raw}" 2>&1 || true
	python3 "${GALE_ROOT}/benches/flight_control/tag_events.py" \
	    "${raw}" "${run_id}" "${name}" >>"${events}"
	local raw_lines
	raw_lines=$(wc -l <"${raw}" 2>/dev/null || echo 0)
	echo "==> ran ${name} run ${run_id}: ${raw_lines} raw lines → ${events}"
}

main() {
	build baseline
	build gale

	: > "${BUILD_ROOT}/flight-baseline/events.csv"
	: > "${BUILD_ROOT}/flight-gale/events.csv"

	for r in $(seq 1 "${RUNS}"); do
		run_one baseline "${r}"
		run_one gale     "${r}"
	done

	local b="${BUILD_ROOT}/flight-baseline/events.csv"
	local g="${BUILD_ROOT}/flight-gale/events.csv"

	echo ""
	echo "==> analyze:"
	python3 "${GALE_ROOT}/benches/flight_control/analyze.py" \
	    --baseline "${b}" --gale "${g}" --runs "${RUNS}"
}

main "$@"
