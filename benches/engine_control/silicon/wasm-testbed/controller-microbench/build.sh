#!/usr/bin/env bash
# controller_step (7-arg value fn) ARM-silicon microbench. synth passes args in r0-r7;
# ctl_tramp.S shuffles AAPCS (r0-r3 + stack) into r0-r7. Result: synth=169/native=61=2.77x (G474RE).
set -euo pipefail
SYNTH="${SYNTH:-synth}"; CLANG=/opt/homebrew/opt/llvm/bin/clang; WASMLD=/opt/homebrew/bin/wasm-ld
TC=/Volumes/Home/zephyr-sdk/zephyr-sdk-1.0.1/gnu/arm-zephyr-eabi/bin/arm-zephyr-eabi
WT=$(cd "$(dirname "$0")/.." && pwd); HERE=$(cd "$(dirname "$0")" && pwd)
t=$(mktemp -d)
$CLANG --target=wasm32-unknown-unknown -O2 -nostdlib -c "$WT/controller_wasm.c" -o "$t/c.o"
$WASMLD --no-entry --export=controller_step_decide --allow-undefined --gc-sections "$t/c.o" -o "$t/c.wasm"
loom optimize "$t/c.wasm" --passes inline --attestation false -o "$t/c.loom.wasm" >/dev/null
"$SYNTH" compile "$t/c.loom.wasm" --target cortex-m4f --all-exports --relocatable -o "$t/ctl.o"
"$TC-objcopy" --redefine-sym controller_step_decide=synth_ctl_body "$t/ctl.o" "$t/ctl_body.o"
"$TC-gcc" -mcpu=cortex-m4 -mthumb -c "$HERE/ctl_tramp.S" -o "$t/ctl_tramp.o"
"$TC-ar" rcs "$t/libctl.a" "$t/ctl_body.o" "$t/ctl_tramp.o"
export ZEPHYR_BASE=/Volumes/Home/git/pulseengine/zephyr ZEPHYR_SDK_INSTALL_DIR=/Volumes/Home/zephyr-sdk/zephyr-sdk-1.0.1
west build -b nucleo_g474re -d "$t/build" -p always "$HERE" -- -DGALE_CTL_LIB="$t/libctl.a" >/dev/null 2>&1
echo "built: $t/build/zephyr/zephyr.elf  (flash + capture for E,controller_step,synth=N,native=M)"
