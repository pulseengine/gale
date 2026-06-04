#!/usr/bin/env bash
# One-command k_mutex_unlock wasm-cross-LTO re-measurement, staged for synth v0.11.29
# (the --native-pointer-abi flag from synth#237). Encodes the full dissolution +
# trampoline + microbench + silicon-capture recipe so the turnaround is instant.
#
# Pipeline (per NOTES-wasm-cross-lto-spike.md):
#   clang wasm_mutex_shim_poc.c -> wasm-ld + libgale_ffi.a -> loom inline ->
#   synth compile --native-pointer-abi --all-exports --relocatable  (v0.11.29+: emits
#     wasm statics as MOVW/MOVT __synth_wasm_data .data relocs; host k_mutex* stays base=0)
#   -> objcopy rename imports->gale_w_*, body->synth_k_mutex_unlock_body
#   -> + mtx_tramp.S (mov r11,#0 ; bl synth_body) -> ar libwasmmutex.a
#   -> west build mutex-microbench (native + GALE_WASM_LTO_MUTEX_LIB) -> flash -> capture
# Reference (gale rustc-direct, native): k_mutex_unlock = 124 cyc (DWT min/200, uncontended).
set -euo pipefail
SYNTH="${SYNTH:-synth}"; CLANG=/opt/homebrew/opt/llvm/bin/clang; WASMLD=/opt/homebrew/bin/wasm-ld
TC=/Volumes/Home/zephyr-sdk/zephyr-sdk-1.0.1/gnu/arm-zephyr-eabi/bin/arm-zephyr-eabi
GR=/Volumes/Home/git/pulseengine/gale-smart-data
SHIM="$GR/benches/engine_control/silicon/boards/nucleo_g474re/wasm_mutex_shim_poc.c"
LIBFFI=/Volumes/Home/git/pulseengine/gale/ffi/target/wasm32-unknown-unknown/release/libgale_ffi.a
VCP="${VCP:-/dev/cu.usbmodem132203}"   # VCP autodetect = lex-first; do NOT pass --port

"$SYNTH" compile --help 2>&1 | grep -q 'native-pointer-abi' || { echo "synth lacks --native-pointer-abi (need v0.11.29+, synth#237); aborting"; exit 3; }
t=$(mktemp -d); cd "$t"
$CLANG --target=wasm32-unknown-unknown -O2 -nostdlib -c "$SHIM" -o shim.o
( cd /Volumes/Home/git/pulseengine/gale/ffi && cargo rustc --release --target wasm32-unknown-unknown --crate-type=staticlib >/dev/null 2>&1 )
$WASMLD --no-entry --export=z_impl_k_mutex_unlock --allow-undefined --gc-sections shim.o "$LIBFFI" -o m.wasm
loom optimize m.wasm --passes inline --attestation false -o m.loom.wasm >/dev/null
"$SYNTH" compile m.loom.wasm --target cortex-m4f --native-pointer-abi --all-exports --relocatable -o m.o
"$TC-objcopy" \
  --redefine-sym k_spin_lock=gale_w_spin_lock --redefine-sym k_spin_unlock=gale_w_spin_unlock \
  --redefine-sym z_unpend_first_thread=gale_w_unpend_first_thread --redefine-sym z_ready_thread=gale_w_ready_thread \
  --redefine-sym arch_thread_return_value_set=gale_w_arch_thread_return_value_set --redefine-sym z_reschedule=gale_w_reschedule \
  --redefine-sym z_impl_k_mutex_unlock=synth_k_mutex_unlock_body  m.o body.o
cat > tramp.S <<'ASM'
	.syntax unified
	.thumb
	.section .text.gale_mtx_tramp,"ax",%progbits
	.global z_impl_k_mutex_unlock
	.thumb_func
z_impl_k_mutex_unlock:
	push {r11, lr}
	mov.w r11, #0
	bl synth_k_mutex_unlock_body
	pop {r11, pc}
ASM
"$TC-gcc" -mcpu=cortex-m4 -mthumb -c tramp.S -o tramp.o
"$TC-ar" rcs "$t/libwasmmutex.a" body.o tramp.o
export ZEPHYR_BASE=/Volumes/Home/git/pulseengine/zephyr ZEPHYR_SDK_INSTALL_DIR=/Volumes/Home/zephyr-sdk/zephyr-sdk-1.0.1
MB="$GR/benches/engine_control/silicon/wasm-testbed/mutex-microbench"
west build -b nucleo_g474re -d "$t/mb" -p always "$MB" -- -DZEPHYR_EXTRA_MODULES="$GR" \
  -DOVERLAY_CONFIG="$GR/zephyr/gale_overlay.conf" -DGALE_WASM_LTO_MUTEX_LIB="$t/libwasmmutex.a" >/dev/null 2>&1
CAP="$GR/benches/engine_control/silicon"
for try in 1 2 3 4; do
  python3 "$CAP/capture.py" --port "$VCP" --baud 115200 --sentinel "=== END ===" --timeout 15 --out "$t/cap" >/dev/null 2>&1 & c=$!; sleep 2
  openocd -f interface/stlink.cfg -f target/stm32g4x.cfg -c "program $t/mb/zephyr/zephyr.elf verify reset exit" >/dev/null 2>&1; wait $c 2>/dev/null
  grep -qaE 'E,k_mutex_unlock|FAULT' "$t/cap" && break
done
echo "=== RESULT ==="; grep -aE 'SELFCHECK|E,k_mutex_unlock|FAULT' "$t/cap" | head
echo "(native gale ref: 124 cyc)"
