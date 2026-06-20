#!/usr/bin/env bash
# RISC-V SYNTH path: wasm→loom→`synth -b riscv`→RV32 on qemu_riscv32. control_step via s11 table trampoline.
# Regenerates control.loom.wasm from shared sources, compiles with the riscv backend, links + runs.
# WORKING: filter_axis (direct) + control_step (s11=&gale_tables-0x10000 tramp). controller/flat_flight pending #226/v0.11.25.
set -u
RV="${RV:-/Volumes/Home/git/zephyr/zephyr-sdk-0.17.4/riscv64-zephyr-elf/bin/riscv64-zephyr-elf-gcc}"
CLANG="${CLANG:-/opt/homebrew/opt/llvm/bin/clang}"; WASMLD="${WASMLD:-/opt/homebrew/bin/wasm-ld}"
QEMU="${QEMU:-/opt/homebrew/bin/qemu-system-riscv32}"; SRC=../..; t=$(mktemp -d)
synth riscv-runtime -t rv32imac --flash-origin 0x80000000 --ram-origin 0x80010000 --linear-memory-size 4096 --stack-size 8192 -o . >/dev/null 2>&1
# dissolve control_step from shared sources -> wasm
$CLANG --target=wasm32-unknown-unknown -O2 -nostdlib -c $SRC/control_wasm.c -o $t/c.o
$CLANG --target=wasm32-unknown-unknown -O2 -nostdlib -c $SRC/tables.c -o $t/tb.o
$WASMLD --no-entry --export=control_step_decide --allow-undefined --gc-sections $t/c.o $t/tb.o -o $t/cs.wasm
loom optimize $t/cs.wasm --passes inline --attestation false -o $t/cs.loom.wasm >/dev/null 2>&1
synth compile $t/cs.loom.wasm -b riscv -t rv32imac --relocatable -o $t/cs_rv.o 2>&1 | grep -iE 'error|skip' && echo "control_step synth -b riscv FAILED (check synth#223/#226)"
$RV -march=rv32imac_zicsr -mabi=ilp32 -O2 -nostartfiles -nostdlib -ffreestanding -T linker.ld \
  -o fw_cs.elf startup.c main_cs.c control_native.c $SRC/tables.c gale_tables.c rv_cs_tramp.S $t/cs_rv.o 2>/dev/null || { echo BUILD-FAIL; exit 1; }
"$QEMU" -machine virt -bios none -nographic -icount shift=0 -kernel fw_cs.elf >/tmp/rvs.txt 2>&1 & p=$!; sleep 5; kill $p 2>/dev/null; wait $p 2>/dev/null
grep -E 'SYNTH-RV32|E,' /tmp/rvs.txt
rm -rf "$t"
echo "(qemu -icount proxy; control_step synth-rv32 via s11 table trampoline)"
