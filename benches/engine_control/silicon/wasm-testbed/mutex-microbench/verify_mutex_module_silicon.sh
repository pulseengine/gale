#!/usr/bin/env bash
# Staged #345 kill-criterion harness: full upstream tests/kernel/mutex/mutex_api on G474RE
# with the wasm-cross-LTO mutex module (CONFIG_GALE_WASM_LTO_MUTEX=y). Run this the moment
# synth#345 (.bss the linmem reservation + PC-relative) lands — one command, instant verdict.
#
# The no-waiter microbench (remeasure_wasm_lto.sh) only exercises the UNLOCKED path; this runs
# the FULL suite, which is what caught the real bugs:
#   - WITH --native-pointer-abi pre-#345: test_complex_inversion USAGE FAULT (MOVW_ABS link
#     corruption from the 64 KB .data) — see synth#345 / gale PR #60.
#   - WITHOUT --native-pointer-abi: test_complex_inversion PASSES but test_mutex_recursive MPU
#     FAULT (host-pointer deref wrong without the flag) — workaround ruled out 2026-06-14.
# KILL-CRITERION (PASS): with --native-pointer-abi on the #345-fixed synth, BOTH
#   test_complex_inversion AND test_mutex_recursive pass, "PROJECT EXECUTION SUCCESSFUL", no FAULT.
set -u
SYNTH="${SYNTH:-synth}"; CLANG="${CLANG:-/opt/homebrew/opt/llvm/bin/clang}"; WASMLD="${WASMLD:-/opt/homebrew/bin/wasm-ld}"
SDK=/Volumes/Home/zephyr-sdk/zephyr-sdk-1.0.1; TC="$SDK/gnu/arm-zephyr-eabi/bin/arm-zephyr-eabi"
GALE=/Volumes/Home/git/pulseengine/gale
export ZEPHYR_BASE=/Volumes/Home/git/pulseengine/zephyr ZEPHYR_SDK_INSTALL_DIR=$SDK
export PATH="$(dirname "$CLANG"):$TC/..:$PATH"
echo "== verify mutex module (synth $($SYNTH --version|head -1|awk '{print $2}'), --native-pointer-abi) =="
t=$(mktemp -d); WD=$(mktemp -d)
# 1. FFI wasm staticlib + shim -> dissolve -> synth (WITH --native-pointer-abi) -> renamed .o
( cd "$GALE/ffi" && cargo rustc --release --target wasm32-unknown-unknown --crate-type=staticlib >/dev/null 2>&1 )
LIBFFI="$GALE/ffi/target/wasm32-unknown-unknown/release/libgale_ffi.a"
$CLANG --target=wasm32-unknown-unknown -O2 -nostdlib -c "$GALE/zephyr/wasm/mutex_unlock_shim.c" -o "$t/shim.o" || exit 2
$WASMLD --no-entry --export=z_impl_k_mutex_unlock --export=gale_k_mutex_unlock_decide --allow-undefined --gc-sections "$LIBFFI" "$t/shim.o" -o "$t/m.wasm" || exit 2
loom optimize "$t/m.wasm" --passes inline --attestation false -o "$t/m.loom.wasm" >/dev/null 2>&1 || exit 2
$SYNTH compile "$t/m.loom.wasm" --target cortex-m4f --native-pointer-abi --all-exports --relocatable -o "$t/m.o" || exit 2
"$TC-objcopy" --redefine-sym k_spin_lock=gale_w_spin_lock --redefine-sym k_spin_unlock=gale_w_spin_unlock \
  --redefine-sym z_unpend_first_thread=gale_w_unpend_first_thread --redefine-sym z_ready_thread=gale_w_ready_thread \
  --redefine-sym arch_thread_return_value_set=gale_w_arch_thread_return_value_set --redefine-sym z_reschedule=gale_w_reschedule \
  --redefine-sym z_impl_k_mutex_unlock=synth_k_mutex_unlock_body "$t/m.o" "$t/r.o"
"$TC-objcopy" --keep-global-symbol=synth_k_mutex_unlock_body "$t/r.o" "$WD/gale-wasm-mutex-cortex-m4f.o"
echo "  module shape: .data=$("$TC-size" -A "$WD/gale-wasm-mutex-cortex-m4f.o" 2>/dev/null|awk '/^\.data/{print $2}')B (target: 0, like sem) MOVW_ABS=$("$TC-objdump" -r "$WD/gale-wasm-mutex-cortex-m4f.o" 2>/dev/null|grep -c MOVW_ABS)"
# 2. build the upstream mutex_api test with the module (override on, branch must have the shim+wiring)
printf "CONFIG_GALE_KERNEL_MUTEX=y\nCONFIG_GALE_WASM_LTO_MUTEX=y\n" > "$t/ov.conf"
west build -b nucleo_g474re -d "$WD/build" "$ZEPHYR_BASE/tests/kernel/mutex/mutex_api" -p always -- \
  -DZEPHYR_EXTRA_MODULES="$GALE" -DOVERLAY_CONFIG="$t/ov.conf" -DGALE_WASM_LTO_MUTEX_OBJ="$WD/gale-wasm-mutex-cortex-m4f.o" >/dev/null 2>&1 \
  || { echo "  [BAD] build failed"; exit 3; }
# 3. flash + capture
openocd -f interface/stlink.cfg -f target/stm32g4x.cfg -c "program $WD/build/zephyr/zephyr.elf verify reset exit" >/dev/null 2>&1
# self-contained capture: VCP autodetect = lex-first /dev/cu.usbmodem* (do NOT hardcode --port);
# reset after opening the port, read until '=== END ===' / PROJECT EXECUTION / timeout.
out=$(python3 - <<'PYCAP'
import serial, glob, time, subprocess, threading
ports=sorted(glob.glob('/dev/cu.usbmodem*'))
ser=serial.Serial(ports[0],115200,timeout=1)
def rst():
    time.sleep(0.5); subprocess.run(["openocd","-f","interface/stlink.cfg","-f","target/stm32g4x.cfg","-c","init;reset;exit"],capture_output=True)
threading.Thread(target=rst,daemon=True).start()
end=time.time()+20; lines=[]
while time.time()<end:
    try: l=ser.readline().decode(errors="replace").rstrip()
    except: continue
    if l: lines.append(l)
    if "PROJECT EXECUTION" in l: break
ser.close(); print("\n".join(lines))
PYCAP
)
echo "$out" | grep -aE "PASS - |FAIL - |FATAL|FAULT|PROJECT EXECUTION" | head -20
if echo "$out" | grep -qaE "PROJECT EXECUTION SUCCESSFUL" && ! echo "$out" | grep -qaE "FAULT|FATAL"; then
  echo "== KILL-CRITERION PASS: mutex module runs the full mutex_api suite on G474RE, no fault =="
else
  echo "== KILL-CRITERION NOT MET: see FAULT/FAIL above (still reproduces) =="
fi
