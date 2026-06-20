#!/usr/bin/env bash
# RV32 FUNCTIONAL regression lane for the wasm-cross-LTO testbed.
# For each dissolved leaf: clang->wasm-ld->loom->`synth compile -b riscv`, link the
# actual RV32 output into a qemu harness, and ASSERT the verified-correct value.
# This is the lane that would have caught synth #232 (filter_axis signed-div
# miscompile on v0.11.26) automatically. Exit 0 = all RV32 outputs correct.
# Tools: clang+wasm-ld, loom, synth, riscv64-*-gcc, qemu-system-riscv32.
set -u
CLANG="${CLANG:-/opt/homebrew/opt/llvm/bin/clang}"; WASMLD="${WASMLD:-/opt/homebrew/bin/wasm-ld}"
RV="${RV:-/Volumes/Home/git/zephyr/zephyr-sdk-0.17.4/riscv64-zephyr-elf/bin/riscv64-zephyr-elf-gcc}"
QEMU="${QEMU:-/opt/homebrew/bin/qemu-system-riscv32}"
SRC=../..; t=$(mktemp -d)
echo "== RV32 funccheck (synth $(synth --version|head -1|awk '{print $2}'), loom $(loom --version|head -1|awk '{print $2}')) =="
synth riscv-runtime -t rv32imac --flash-origin 0x80000000 --ram-origin 0x80010000 \
  --linear-memory-size 4096 --stack-size 8192 -o . >/dev/null 2>&1
dissolve() { # name export src...
  local name="$1" exp="$2"; shift 2; local objs=""
  for s in "$@"; do $CLANG --target=wasm32-unknown-unknown -O2 -nostdlib -c "$SRC/$s" -o "$t/${s%.c}.o" 2>/dev/null || return 1; objs="$objs $t/${s%.c}.o"; done
  $WASMLD --no-entry --export=$exp --allow-undefined --gc-sections $objs -o "$t/$name.wasm" 2>/dev/null || return 1
  loom optimize "$t/$name.wasm" --passes inline --attestation false -o "$t/$name.loom.wasm" >/dev/null 2>&1 || return 1
  synth compile "$t/$name.loom.wasm" -b riscv -t rv32imac --relocatable -o "$t/$name.o" >/dev/null 2>&1 || return 1
}
dissolve filter     filter_axis_decide     filter_wasm.c      || { echo "BUILD-FAIL filter"; exit 1; }
dissolve controller controller_step_decide controller_wasm.c  || { echo "BUILD-FAIL controller"; exit 1; }
dissolve control    control_step_decide    control_wasm.c tables.c || { echo "BUILD-FAIL control"; exit 1; }
$RV -march=rv32imac_zicsr -mabi=ilp32 -O2 -nostartfiles -nostdlib -ffreestanding -T linker.ld -I. \
  -o fw_funccheck.elf startup.c funccheck_main.c rv_cs_tramp.S gale_tables.c \
  "$t/filter.o" "$t/controller.o" "$t/control.o" 2>/dev/null || { echo BUILD-FAIL link; exit 1; }
"$QEMU" -machine virt -bios none -nographic -icount shift=0 -kernel fw_funccheck.elf >/tmp/rv_fc.txt 2>&1 & p=$!
sleep 5; kill $p 2>/dev/null; wait $p 2>/dev/null
grep -E 'PASS|FAIL' /tmp/rv_fc.txt
rm -rf "$t"
if grep -q FAIL /tmp/rv_fc.txt; then echo "== RV32 FUNCCHECK: FAILURES (synth RV32 miscompile) =="; exit 1
elif grep -q 'PASS control_step' /tmp/rv_fc.txt; then echo "== RV32 FUNCCHECK: ALL GREEN =="; exit 0
else echo "== RV32 FUNCCHECK: no output (qemu/capture issue) =="; exit 2; fi
