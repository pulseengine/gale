#!/usr/bin/env bash
# flat_flight (composed flight algo) ARM-silicon microbench = Track-A frozen regression target (#209).
# Buffer-harness (fp=&wasm_linmem): flat_flight takes 2 wasm-linmem ptrs + a linmem stack; 0 statics.
# Baseline (loom 1.1.10 + synth 0.11.30): synth=262 / native=103 = 2.54x (G474RE, SELFCHECK 0x07fdf307).
set -euo pipefail
SYNTH="${SYNTH:-synth}"; CLANG=/opt/homebrew/opt/llvm/bin/clang; WASMLD=/opt/homebrew/bin/wasm-ld
TC=/Volumes/Home/zephyr-sdk/zephyr-sdk-1.0.1/gnu/arm-zephyr-eabi/bin/arm-zephyr-eabi
WT=$(cd "$(dirname "$0")/.." && pwd); HERE=$(cd "$(dirname "$0")" && pwd); t=$(mktemp -d)
$CLANG --target=wasm32-unknown-unknown -O2 -nostdlib -I"$WT/arch/riscv" -c "$WT/flat_flight.c" -o "$t/ff.o"
$WASMLD --no-entry --export=flat_flight --allow-undefined --gc-sections "$t/ff.o" -o "$t/ff.wasm"
loom optimize "$t/ff.wasm" --passes inline --attestation false -o "$t/ff.loom.wasm" >/dev/null
"$SYNTH" compile "$t/ff.loom.wasm" --target cortex-m4f --all-exports --relocatable -o "$t/ff.o2"
"$TC-objcopy" --redefine-sym flat_flight=synth_flat_flight_body "$t/ff.o2" "$t/ff_body.o"
"$TC-gcc" -mcpu=cortex-m4 -mthumb -c "$HERE/ff_tramp.S" -o "$t/ff_tramp.o"
"$TC-ar" rcs "$t/libff.a" "$t/ff_body.o" "$t/ff_tramp.o"
export ZEPHYR_BASE=/Volumes/Home/git/pulseengine/zephyr ZEPHYR_SDK_INSTALL_DIR=/Volumes/Home/zephyr-sdk/zephyr-sdk-1.0.1
west build -b nucleo_g474re -d "$t/build" -p always "$HERE" -- -DGALE_FF_LIB="$t/libff.a" >/dev/null 2>&1
echo "built: $t/build/zephyr/zephyr.elf  (flash + capture for E,flat_flight,synth=N,native=M)"
