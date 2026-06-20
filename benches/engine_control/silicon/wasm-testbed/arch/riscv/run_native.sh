#!/usr/bin/env bash
# RISC-V arch adapter — NATIVE baseline on qemu_riscv32 (the "differs" back-half for RISC-V).
# SHARED inputs (from ../../): the algorithm C sources (control_wasm.c, filter_wasm.c, flat_flight.c,
# tables.c, control.h) and the verified vectors. DIFFERS (here): synth riscv-runtime (startup.c+linker.ld),
# the mcycle-CSR harness (main.c), and qemu-system-riscv32. For the synth-rv32 path, compile the SAME
# dissolved wasm with `synth compile <fn>.loom.wasm -b riscv -t rv32imac` and link its .o in place of the
# native obj (s11 = linear-memory base; trampoline analogous to ARM's r11).
set -u
RV="${RV:-/Volumes/Home/git/zephyr/zephyr-sdk-0.17.4/riscv64-zephyr-elf/bin/riscv64-zephyr-elf-gcc}"
QEMU="${QEMU:-/opt/homebrew/bin/qemu-system-riscv32}"
SRC=../..
synth riscv-runtime -t rv32imac --flash-origin 0x80000000 --ram-origin 0x80010000 \
  --linear-memory-size 4096 --stack-size 8192 -o . >/dev/null 2>&1
$RV -march=rv32imac_zicsr -mabi=ilp32 -O2 -nostartfiles -nostdlib -ffreestanding -T linker.ld -I. \
  -o fw.elf startup.c main.c $SRC/filter_wasm.c $SRC/control_wasm.c $SRC/tables.c $SRC/flat_flight.c || { echo BUILD-FAIL; exit 1; }
$QEMU -machine virt -bios none -nographic -icount shift=0 -kernel fw.elf >/tmp/rv_run.txt 2>&1 & QP=$!
sleep 5; kill $QP 2>/dev/null; wait $QP 2>/dev/null
grep -E 'E,|RV-NATIVE' /tmp/rv_run.txt
echo "(qemu -icount = instruction-count proxy, NOT silicon cycles; real RV cycles via ESP32-C3/Renode)"
