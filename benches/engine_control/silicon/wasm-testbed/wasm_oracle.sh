#!/usr/bin/env bash
# Host-side wasm functional oracle for the engine_control algorithm.
# Runs control_step_decide in wasmtime and checks against the native reference.
# Part of the "test environment as wasm" testbed: lets us verify functional
# correctness of any synth/loom input WITHOUT hardware.
#
# NOTE: wasmtime --invoke must be called one-shot per process; capturing it in a
# shell loop subshell came back empty in testing, so we run each vector explicitly.
set -u
WASM="${1:-control.loom.wasm}"
EXPECT=(2164988 2165333 2165678 2230501)   # native reference, packed spark<<16|fuel
ARGS=("3000 50 90 0" "3000 50 40 0" "3000 50 0 0" "6000 80 40 3")
fail=0
for i in 0 1 2 3; do
  got=$(wasmtime --invoke control_step_decide "$WASM" ${ARGS[$i]} 2>/dev/null)
  exp=${EXPECT[$i]}
  spark=$(( (got>>16) & 0xffff )); fuel=$(( got & 0xffff ))
  if [ "$got" = "$exp" ]; then mark="OK "; else mark="BAD"; fail=1; fi
  printf "[%s] (%s) got=%s exp=%s  spark=%d fuel=%d\n" "$mark" "${ARGS[$i]}" "$got" "$exp" "$spark" "$fuel"
done
echo "--- controller_step_decide (flight_control) ---"
CEXP=(0x00000000 0x05ce7f9c 0x07000081 0xff7fc27f)   # native reference build of controller_step
CARGS=("0 0 0 0 0 0 0" "6400 0 -12800 0 3200 0 5" "100000 0 0 0 0 0 7" "-100000 256 4096 -256 -8192 128 255")
for i in 0 1 2 3; do
  got=$(wasmtime --invoke controller_step_decide controller.loom.wasm ${CARGS[$i]} 2>/dev/null)
  gotu=$(printf "0x%08x" $(( got & 0xffffffff ))); exp=${CEXP[$i]}
  if [ "$gotu" = "$exp" ]; then m="OK "; else m="BAD"; fail=1; fi
  printf "[%s] (%s) got=%s exp=%s\n" "$m" "${CARGS[$i]}" "$gotu" "$exp"
done
echo "--- filter_axis_decide (flight_control) ---"
FEXP=(0 1088 -1917 1); FARGS=("0 0 0" "1000 100 500" "-2000 50 -300" "0 2 0")
for i in 0 1 2 3; do
  got=$(wasmtime --invoke filter_axis_decide filter.loom.wasm ${FARGS[$i]} 2>/dev/null)
  if [ "$got" = "${FEXP[$i]}" ]; then m="OK "; else m="BAD"; fail=1; fi
  printf "[%s] (%s) got=%s exp=%s\n" "$m" "${FARGS[$i]}" "$got" "${FEXP[$i]}"
done
[ $fail -eq 0 ] && echo "ALL PASS" || echo "MISMATCH"
exit $fail
