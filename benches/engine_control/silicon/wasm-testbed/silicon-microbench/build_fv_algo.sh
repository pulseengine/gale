#!/usr/bin/env bash
# Rebuild /tmp/wasm-algo-poc/fv_algo.o (the 3-variant filter decomposition object
# linked by CMakeLists.txt). /tmp gets cleaned by macOS — rerun this when it does.
# SYNTH_RANGE_REALLOC=1 may be exported to build the flag-on variant.
set -euo pipefail
SYNTH="${SYNTH:-synth}"; CLANG=/opt/homebrew/opt/llvm/bin/clang; WASMLD=/opt/homebrew/bin/wasm-ld
TC=/Volumes/Home/zephyr-sdk/zephyr-sdk-1.0.1/gnu/arm-zephyr-eabi/bin/arm-zephyr-eabi
HERE=$(cd "$(dirname "$0")" && pwd); mkdir -p /tmp/wasm-algo-poc; cd /tmp/wasm-algo-poc
$CLANG --target=wasm32-unknown-unknown -O2 -nostdlib -c "$HERE/fv_algo.c" -o fv.o
$WASMLD --no-entry --export=full --export=nodiv --export=div --allow-undefined --gc-sections fv.o -o fv.wasm
loom optimize fv.wasm --passes inline --attestation false -o fv.loom.wasm >/dev/null
"$SYNTH" compile fv.loom.wasm --target cortex-m4f --all-exports --relocatable -o fv_algo.raw.o
"$TC-objcopy" --redefine-sym full=v_full --redefine-sym nodiv=v_nodiv --redefine-sym div=v_div fv_algo.raw.o fv_algo.o
echo "wrote /tmp/wasm-algo-poc/fv_algo.o (synth $($SYNTH --version | awk '{print $2}'))"
