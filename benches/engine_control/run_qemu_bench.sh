#!/usr/bin/env bash
# Engine-control benchmark — QEMU event-stream runner with regression
# asserts.
#
# Post-#25 methodology: the firmware emits raw per-ISR event lines
# (E,<seq>,<step>,<rpm>,<algo_cycles>,<handoff_cycles>). All statistics
# are computed off-target by analyze.py. This script:
#
#   1. Builds baseline + gale variants
#   2. Runs each N times (default 1 for CI smoke; -n 20 for manual
#      statistical power)
#   3. Concatenates per-run event streams with a run-id prefix
#   4. Invokes analyze.py which asserts:
#        - sample count >= expected (no truncation)
#        - drops == 0
#        - algo distributions overlap across builds > 95% (integrity:
#          same C code should produce the same algorithm timing)
#        - no handoff sample exceeds 2x baseline p99 (regression guard)
#      and prints a median + 95% bootstrap CI comparison table with
#      Mann-Whitney U p-values per RPM step.
#
# Usage:
#   ./run_qemu_bench.sh [-n RUNS]
#
# Env (defaults for local z/ layout):
#   ZEPHYR_BASE, ZEPHYR_SDK_INSTALL_DIR, GALE_ROOT, BUILD_ROOT

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

# Per-run QEMU timeout. Firmware reaches `=== END ===` in ~30s for the
# short sweep; QEMU doesn't halt on main-return (qemu_cortex_m3 has no
# semihost-exit path in this Zephyr build), so we rely on SIGTERM from
# `timeout` to stop the emulator. 60s covers the run + a healthy safety
# margin. Bump for the long sweep (7750 samples) if you adapt this script
# for stm32f4_disco. Renode has its own Robot timeout.
readonly PER_RUN_TIMEOUT=60

build() {
	local name="$1"   # baseline | gale
	local dir="${BUILD_ROOT}/engine-${name}"
	local extra=()
	if [ "${name}" = "gale" ]; then
		extra+=( -DZEPHYR_EXTRA_MODULES="${GALE_ROOT}" )
		extra+=( -DOVERLAY_CONFIG="${GALE_ROOT}/benches/engine_control/prj-gale.conf" )
	fi
	rm -rf "${dir}"
	west build -b qemu_cortex_m3 -d "${dir}" \
	    -s "${GALE_ROOT}/benches/engine_control" \
	    ${extra[@]+"--"} ${extra[@]+"${extra[@]}"} \
	  >"${dir}.build.log" 2>&1
	echo "==> built ${name}: ${dir}/zephyr/zephyr.elf"
}

run_one() {
	# run_one <variant> <run_id> → appends to ${dir}/events.csv
	local name="$1" run_id="$2"
	local dir="${BUILD_ROOT}/engine-${name}"
	local raw="${dir}/run${run_id}.raw"
	local events="${dir}/events.csv"
	timeout "${PER_RUN_TIMEOUT}" west build -d "${dir}" -t run \
	    >"${raw}" 2>&1 || true
	# Append to the combined events CSV with a run-id prefix on each
	# event line. The analyzer uses this to distinguish runs while
	# still pooling distributions across them.
	python3 "${GALE_ROOT}/benches/engine_control/tag_events.py" \
	    "${raw}" "${run_id}" "${name}" >>"${events}"
	local raw_lines
	raw_lines=$(wc -l <"${raw}" 2>/dev/null || echo 0)
	echo "==> ran ${name} run ${run_id}: ${raw_lines} raw lines → ${events}"
}

main() {
	build baseline
	build gale

	# Fresh combined event files
	: > "${BUILD_ROOT}/engine-baseline/events.csv"
	: > "${BUILD_ROOT}/engine-gale/events.csv"

	for r in $(seq 1 "${RUNS}"); do
		run_one baseline "${r}"
		run_one gale     "${r}"
	done

	local b="${BUILD_ROOT}/engine-baseline/events.csv"
	local g="${BUILD_ROOT}/engine-gale/events.csv"

	echo ""
	echo "==> analyze:"
	python3 "${GALE_ROOT}/benches/engine_control/analyze.py" \
	    --baseline "${b}" --gale "${g}" --runs "${RUNS}"
}

main "$@"
