#!/usr/bin/env bash
# control_step (engine algo, 4 scalar args + 2 lookup tables) ARM-silicon microbench.
# Tables live in wasm linmem at offset 65536 (spark 400B) + 65936 (fuel 800B); the harness
# copies them into a RAM buffer and the r11=&buffer trampoline addresses them ([r11+offset]).
# Current (loom 1.1.10 + synth 0.11.33): synth=156 / native=67 = 2.33x (G474RE, SELFCHECK 2165333).
set -euo pipefail
SYNTH="${SYNTH:-synth}"; CLANG=/opt/homebrew/opt/llvm/bin/clang; WASMLD=/opt/homebrew/bin/wasm-ld
TC=/Volumes/Home/zephyr-sdk/zephyr-sdk-1.0.1/gnu/arm-zephyr-eabi/bin/arm-zephyr-eabi
WT=$(cd "$(dirname "$0")/.." && pwd); HERE=$(cd "$(dirname "$0")" && pwd); t=$(mktemp -d)
$CLANG --target=wasm32-unknown-unknown -O2 -nostdlib -c "$WT/control_wasm.c" -o "$t/cs.o"
$CLANG --target=wasm32-unknown-unknown -O2 -nostdlib -c "$WT/tables.c" -o "$t/tb.o"
$WASMLD --no-entry --export=control_step_decide --allow-undefined --gc-sections "$t/cs.o" "$t/tb.o" -o "$t/cs.wasm"
loom optimize "$t/cs.wasm" --passes inline --attestation false -o "$t/cs.loom.wasm" >/dev/null
"$SYNTH" compile "$t/cs.loom.wasm" --target cortex-m4f --all-exports --relocatable -o "$t/cs.o2"
"$TC-objcopy" --redefine-sym control_step_decide=synth_cs_body "$t/cs.o2" "$t/cs_body.o"
"$TC-gcc" -mcpu=cortex-m4 -mthumb -c "$HERE/cs_tramp.S" -o "$t/cs_tramp.o"
"$TC-gcc" -mcpu=cortex-m4 -mthumb -O2 -c "$WT/control_wasm.c" -Dcontrol_step_decide=n_control_step -o "$t/cs_native.o"
"$TC-gcc" -mcpu=cortex-m4 -mthumb -O2 -c "$WT/tables.c" -o "$t/cs_tables.o"
"$TC-ar" rcs "$t/libcs.a" "$t/cs_body.o" "$t/cs_tramp.o" "$t/cs_native.o" "$t/cs_tables.o"
export ZEPHYR_BASE=/Volumes/Home/git/pulseengine/zephyr ZEPHYR_SDK_INSTALL_DIR=/Volumes/Home/zephyr-sdk/zephyr-sdk-1.0.1
west build -b nucleo_g474re -d "$t/build" -p always "$HERE" -- -DGALE_CS_LIB="$t/libcs.a" >/dev/null 2>&1
echo "built: $t/build/zephyr/zephyr.elf  (flash + capture for E,control_step,synth=N,native=M)"
