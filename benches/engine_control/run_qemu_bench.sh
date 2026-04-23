#!/usr/bin/env bash
# Engine-control benchmark — QEMU smoke run with regression asserts.
#
# Runs the short (150-sample) version on qemu_cortex_m3 in both
# baseline and gale configurations and asserts:
#   - both builds succeed
#   - both runs complete (produce "=== END ===")
#   - neither run reports drops > 0
#   - algo-mean matches across builds within 5% (measurement integrity)
#   - handoff-mean is below a conservative ceiling
#
# Intended for CI: fast (~3 min in a warm container) and reliably
# catches regressions in the ISR/primitive path without requiring
# real hardware or Renode.
#
# Usage:
#   scripts/dev-build-test.sh-style env required:
#     ZEPHYR_BASE, ZEPHYR_SDK_INSTALL_DIR, GALE_ROOT (defaults if in
#     the standard z/ workspace layout)
#   ./run_qemu_bench.sh

set -euo pipefail

: "${ZEPHYR_WORKSPACE:=/Users/r/git/pulseengine/z}"
: "${ZEPHYR_BASE:=${ZEPHYR_WORKSPACE}/zephyr}"
: "${ZEPHYR_SDK_INSTALL_DIR:=/Users/r/zephyr-sdk-1.0.1}"
: "${GALE_ROOT:=${ZEPHYR_WORKSPACE}/gale}"
: "${BUILD_ROOT:=/tmp}"

export ZEPHYR_BASE ZEPHYR_SDK_INSTALL_DIR

# Regression thresholds — tune as data accumulates.
readonly MAX_DROPS=0
readonly MAX_HANDOFF_MEAN_CYCLES=800   # ~67 μs at 12 MHz QEMU; very loose
readonly ALGO_MATCH_PCT=10             # algo mean deltas above this fail

build() {
	local name="$1"   # baseline | gale
	local dir="${BUILD_ROOT}/engine-${name}"
	local extra=()
	if [ "${name}" = "gale" ]; then
		extra+=( -DZEPHYR_EXTRA_MODULES="${GALE_ROOT}" )
		extra+=( -DOVERLAY_CONFIG="${GALE_ROOT}/benches/engine_control/prj-gale.conf" )
	fi
	rm -rf "${dir}"
	# Absolute -s path so this script works in any workspace layout
	# (local z/ vs CI $GITHUB_WORKSPACE/gale). west still needs to
	# find a zephyr installation via ZEPHYR_BASE.
	west build -b qemu_cortex_m3 -d "${dir}" \
	    -s "${GALE_ROOT}/benches/engine_control" \
	    ${extra[@]+"--"} ${extra[@]+"${extra[@]}"} \
	  >"${dir}.build.log" 2>&1
	echo "==> built ${name}: ${dir}/zephyr/zephyr.elf"
}

run() {
	local name="$1"
	local dir="${BUILD_ROOT}/engine-${name}"
	local csv="${dir}/output.csv"
	timeout 90 west build -d "${dir}" -t run >"${csv}" 2>&1 || true
	echo "==> ran ${name}: ${csv}"
}

field() {
	# field <csv> <key> — returns last comma-separated value, CR-stripped.
	# QEMU UART output sprinkles \r into the console, so awk's $NF can
	# include one; tr -d '\r' before the integer-comparison tests is
	# what keeps macOS /bin/sh tests from erroring with "expected int".
	local csv="$1" key="$2"
	awk -F, -v k="${key}" '$1==k {print $NF; exit}' "${csv}" | tr -d '\r\n'
}

sub_field() {
	# e.g. sub_field <csv> algo mean
	local csv="$1" tag="$2" metric="$3"
	awk -F, -v t="${tag}" -v m="${metric}" \
	    '$1==t && $2==m {print $3; exit}' "${csv}" | tr -d '\r\n'
}

assert() {
	local label="$1" cond="$2" expected="$3"
	if ! eval "${cond}"; then
		echo "FAIL [${label}]: ${cond} (expected ${expected})"
		return 1
	fi
	echo "pass [${label}]"
}

main() {
	build baseline
	build gale
	run   baseline
	run   gale

	local b="${BUILD_ROOT}/engine-baseline/output.csv"
	local g="${BUILD_ROOT}/engine-gale/output.csv"

	# End marker present → run completed (not just hung).
	grep -q "=== END ===" "${b}" || { echo "FAIL: baseline did not finish"; tail -20 "${b}"; exit 1; }
	grep -q "=== END ===" "${g}" || { echo "FAIL: gale did not finish"; tail -20 "${g}"; exit 1; }

	local b_drops g_drops b_algo g_algo b_handoff g_handoff
	b_drops=$(field "${b}"    drops)
	g_drops=$(field "${g}"    drops)
	b_algo=$(sub_field "${b}" algo    mean)
	g_algo=$(sub_field "${g}" algo    mean)
	b_handoff=$(sub_field "${b}" handoff mean)
	g_handoff=$(sub_field "${g}" handoff mean)

	echo ""
	echo "baseline: drops=${b_drops} algo_mean=${b_algo} handoff_mean=${b_handoff}"
	echo "gale:     drops=${g_drops} algo_mean=${g_algo} handoff_mean=${g_handoff}"
	echo ""

	local rc=0
	assert "baseline.drops<=${MAX_DROPS}" "[ ${b_drops} -le ${MAX_DROPS} ]" "<=${MAX_DROPS}" || rc=1
	assert "gale.drops<=${MAX_DROPS}"     "[ ${g_drops} -le ${MAX_DROPS} ]" "<=${MAX_DROPS}" || rc=1
	assert "baseline.handoff_mean<${MAX_HANDOFF_MEAN_CYCLES}" \
	       "[ ${b_handoff} -lt ${MAX_HANDOFF_MEAN_CYCLES} ]" "<${MAX_HANDOFF_MEAN_CYCLES}" || rc=1
	assert "gale.handoff_mean<${MAX_HANDOFF_MEAN_CYCLES}" \
	       "[ ${g_handoff} -lt ${MAX_HANDOFF_MEAN_CYCLES} ]" "<${MAX_HANDOFF_MEAN_CYCLES}" || rc=1

	# Algo mean delta within ALGO_MATCH_PCT. abs(g - b) / b * 100 < pct.
	local diff=$(( g_algo > b_algo ? g_algo - b_algo : b_algo - g_algo ))
	local pct=$(( diff * 100 / b_algo ))
	assert "algo.match<${ALGO_MATCH_PCT}%" "[ ${pct} -lt ${ALGO_MATCH_PCT} ]" "<${ALGO_MATCH_PCT}%" || rc=1

	echo ""
	python3 "${GALE_ROOT}/benches/engine_control/compare.py" "${b}" "${g}"

	exit "${rc}"
}

main "$@"
