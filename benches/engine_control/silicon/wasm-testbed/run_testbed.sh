#!/usr/bin/env bash
# Self-contained wasm-cross-LTO testbed / regression suite.
# For each algorithm function, from source in this dir:
#   1. clang -> wasm-ld -> loom (dissolve)  -> wasm module
#   2. FUNCTIONAL: wasmtime --invoke each vector vs the verified native-reference expected
#   3. CODEGEN:    synth compile (cortex-m4f, --relocatable) must succeed (catches the
#                  v0.11.19-class register-exhaustion / skip regressions)
#
# Run on every new synth/loom release: `./run_testbed.sh`. Exit 0 = all green.
# Verified-correct expected values are ground truth (native gcc == wasmtime == on-silicon,
# established across the 2026-06-02 silicon runs; see ../runs/ and the spike NOTES).
#
# Tools: clang+wasm-ld (llvm), loom, synth, wasmtime on PATH.
set -u
CLANG="${CLANG:-/opt/homebrew/opt/llvm/bin/clang}"
WASMLD="${WASMLD:-/opt/homebrew/bin/wasm-ld}"
TGT=cortex-m4f
fail=0; tmp=$(mktemp -d)

build() { # name exports-csv  src...
  local name="$1"; shift; local exports="$1"; shift
  local objs=""
  for s in "$@"; do local o="$tmp/${s%.c}.o"; "$CLANG" --target=wasm32-unknown-unknown -O2 -nostdlib -c "$s" -o "$o" 2>/dev/null || return 1; objs="$objs $o"; done
  local exp_args=""; for e in ${exports//,/ }; do exp_args="$exp_args --export=$e"; done
  "$WASMLD" --no-entry $exp_args --allow-undefined --gc-sections $objs -o "$tmp/$name.wasm" 2>/dev/null || return 1
  loom optimize "$tmp/$name.wasm" --passes inline --attestation false -o "$tmp/$name.loom.wasm" >/dev/null 2>&1 || return 1
}
codegen_ok() { # name -> synth compiles all exports?
  local out; out=$(synth compile "$tmp/$1.loom.wasm" --target $TGT --all-exports --relocatable -o "$tmp/$1.o" 2>&1)
  if echo "$out" | grep -qiE 'register exhaustion|no functions compiled|skipping function'; then
    echo "    codegen: $(echo "$out" | grep -iE 'exhaustion|skipping' | head -1 | sed 's/^ *//')"; return 1; fi
  return 0
}
inv() { local fn="$1"; local args="$2"; wasmtime --invoke "$fn" "$tmp/$fn.loom.wasm" $args 2>/dev/null; }
chk() { if [ "$2" = "$3" ]; then echo "  [OK ] $1 = $2"; else echo "  [BAD] $1: got='$2' exp='$3'"; fail=1; fi; }

echo "== wasm-cross-LTO testbed (synth $(synth --version|head -1|awk '{print $2}'), loom $(loom --version|head -1|awk '{print $2}')) =="

echo "control_step_decide (engine algo: 4 unsigned-const divides + 2 tables):"
if build control_step_decide control_step_decide control_wasm.c tables.c; then
  codegen_ok control_step_decide && echo "  [OK ] synth compiles" || { echo "  [BAD] synth codegen"; fail=1; }
  chk "(3000,50,90,0)" "$(inv control_step_decide '3000 50 90 0')" 2164988
  chk "(3000,50,40,0)" "$(inv control_step_decide '3000 50 40 0')" 2165333
  chk "(3000,50,0,0)"  "$(inv control_step_decide '3000 50 0 0')"  2165678
  chk "(6000,80,40,3)" "$(inv control_step_decide '6000 80 40 3')" 2230501
else echo "  [BAD] build failed"; fail=1; fi

echo "controller_step_decide (flight controller: SAR + saturation pack):"
if build controller_step_decide controller_step_decide controller_wasm.c; then
  codegen_ok controller_step_decide && echo "  [OK ] synth compiles" || { echo "  [BAD] synth codegen"; fail=1; }
  chk "(6400,0,-12800,0,3200,0,5)" "$(inv controller_step_decide '6400 0 -12800 0 3200 0 5')" 97419164  # 0x05ce7f9c
  chk "(0,0,0,0,0,0,0)"            "$(inv controller_step_decide '0 0 0 0 0 0 0')"            0
else echo "  [BAD] build failed"; fail=1; fi

echo "filter_axis_decide (flight filter: signed mul + signed-const divide):"
if build filter_axis_decide filter_axis_decide filter_wasm.c; then
  codegen_ok filter_axis_decide && echo "  [OK ] synth compiles" || { echo "  [BAD] synth codegen"; fail=1; }
  chk "(0,0,0)"         "$(inv filter_axis_decide '0 0 0')"         0
  chk "(1000,100,500)"  "$(inv filter_axis_decide '1000 100 500')"  1088
  chk "(-2000,50,-300)" "$(inv filter_axis_decide '-2000 50 -300')" -1917
else echo "  [BAD] build failed"; fail=1; fi

rm -rf "$tmp"
echo "== $( [ $fail -eq 0 ] && echo 'ALL GREEN' || echo 'FAILURES — see [BAD] above' ) =="
exit $fail
