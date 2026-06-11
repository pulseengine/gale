#!/usr/bin/env python3
"""i64-unpack differential lane (synth#311 regression guard).

Builds u64repro.c through clang->wasm-ld->loom->synth (cortex-m4f), then
checks the SAME call differentially:
  wasmtime  (wasm ground truth)   check(3,4) == 8, check(0,0) == 1
  unicorn   (synth ARM execution) must match

synth#311: the ARM i64 shift/mask unpack materialized its constants into the
live u64 register pair (r0/r1), silently corrupting packed-u64 returns —
k_sem_give dropped its count update and the engine_control bench hung with no
fault. This lane turns that silent wrong-code into a red testbed line.
"""
import glob, os, struct, subprocess, sys, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
TC = "/Volumes/Home/zephyr-sdk/zephyr-sdk-1.0.1/gnu/arm-zephyr-eabi/bin/arm-zephyr-eabi"
CLANG = "/opt/homebrew/opt/llvm/bin/clang"
WASMLD = "/opt/homebrew/bin/wasm-ld"
SYNTH = os.environ.get("SYNTH", "synth")
VECTORS = [((3, 4), 8), ((0, 0), 1), ((1000, 99), 1100), ((0xFFFF, 1), 0x10001)]
RV = "/Volumes/Home/git/zephyr/zephyr-sdk-0.17.4/riscv64-zephyr-elf/bin/riscv64-zephyr-elf"

def run(cmd, **kw):
    r = subprocess.run(cmd, capture_output=True, text=True, **kw)
    if r.returncode != 0:
        raise RuntimeError(f"{' '.join(cmd)}\n{r.stderr[-400:]}")
    return r.stdout

def main():
    t = tempfile.mkdtemp()
    run([CLANG, "--target=wasm32-unknown-unknown", "-O2", "-nostdlib",
         "-c", os.path.join(HERE, "u64repro.c"), "-o", f"{t}/u.o"])
    run([WASMLD, "--no-entry", "--export=check", "--allow-undefined",
         "--gc-sections", f"{t}/u.o", "-o", f"{t}/u.wasm"])
    run(["loom", "optimize", f"{t}/u.wasm", "--passes", "inline",
         "--attestation", "false", "-o", f"{t}/u.loom.wasm"])
    run([SYNTH, "compile", f"{t}/u.loom.wasm", "--target", "cortex-m4f",
         "--all-exports", "--relocatable", "-o", f"{t}/u.arm.o"])
    run([f"{TC}-ld", "-Ttext=0x8000", "--entry=check", f"{t}/u.arm.o", "-o", f"{t}/u.elf"])
    run([f"{TC}-objcopy", "-O", "binary", "--only-section=.text", f"{t}/u.elf", f"{t}/u.bin"])
    entry = next(int(l.split()[0], 16) for l in
                 run([f"{TC}-nm", f"{t}/u.elf"]).splitlines() if l.endswith(" T check"))

    from unicorn import Uc, UC_ARCH_ARM, UC_MODE_THUMB, UcError
    from unicorn.arm_const import (UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R11,
                                   UC_ARM_REG_SP, UC_ARM_REG_LR)
    # RV32 leg (synth#312 guard): same module through -b riscv, executed
    # under unicorn RISCV32. Was a hard selector rejection before v0.11.37.
    rv32_ok = True
    try:
        run([SYNTH, "compile", f"{t}/u.loom.wasm", "-b", "riscv", "-t", "rv32imac",
             "--all-exports", "--relocatable", "-o", f"{t}/u.rv32.o"])
        run([f"{RV}-ld", "-m", "elf32lriscv", "-Ttext=0x80000000", "--entry=check",
             f"{t}/u.rv32.o", "-o", f"{t}/u.rv32.elf"])
        run([f"{RV}-objcopy", "-O", "binary", "--only-section=.text",
             f"{t}/u.rv32.elf", f"{t}/u.rv32.bin"])
        rv_entry = next(int(l.split()[0], 16) for l in
                        run([f"{RV}-nm", f"{t}/u.rv32.elf"]).splitlines()
                        if l.endswith(" T check"))
    except (RuntimeError, StopIteration) as e:
        print(f"FAIL rv32 compile/link: {e}")
        rv32_ok = False

    ok = True
    for (a, b), exp in VECTORS:
        wt = int(run(["wasmtime", "run", "--invoke", "check",
                      f"{t}/u.loom.wasm", str(a), str(b)]).strip() or -1)
        mu = Uc(UC_ARCH_ARM, UC_MODE_THUMB)
        mu.mem_map(0x0, 0x10000); mu.mem_map(0x20000000, 0x10000)
        mu.mem_write(0x8000, open(f"{t}/u.bin", "rb").read())
        mu.reg_write(UC_ARM_REG_R0, a); mu.reg_write(UC_ARM_REG_R1, b)
        mu.reg_write(UC_ARM_REG_R11, 0)
        mu.reg_write(UC_ARM_REG_SP, 0x20008000)
        mu.reg_write(UC_ARM_REG_LR, 0xDEAD0001)
        try:
            mu.emu_start(entry | 1, 0xDEAD0000, timeout=2_000_000, count=10000)
            arm = mu.reg_read(UC_ARM_REG_R0)
        except UcError as e:
            arm = f"FAULT({e})"
        rv = "-"
        if rv32_ok:
            from unicorn import UC_ARCH_RISCV, UC_MODE_RISCV32
            from unicorn.riscv_const import (UC_RISCV_REG_A0, UC_RISCV_REG_A1,
                                             UC_RISCV_REG_SP, UC_RISCV_REG_RA,
                                             UC_RISCV_REG_S11)
            rmu = Uc(UC_ARCH_RISCV, UC_MODE_RISCV32)
            rmu.mem_map(0x80000000, 0x10000)
            rmu.mem_map(0x80100000, 0x10000)
            rmu.mem_map(0x90000000, 0x1000)
            rmu.mem_write(0x80000000, open(f"{t}/u.rv32.bin", "rb").read())
            rmu.reg_write(UC_RISCV_REG_A0, a); rmu.reg_write(UC_RISCV_REG_A1, b)
            rmu.reg_write(UC_RISCV_REG_SP, 0x80108000)
            rmu.reg_write(UC_RISCV_REG_S11, 0)
            rmu.reg_write(UC_RISCV_REG_RA, 0x90000000)
            try:
                rmu.emu_start(rv_entry, 0x90000000, timeout=2_000_000, count=20000)
                rv = rmu.reg_read(UC_RISCV_REG_A0)
            except UcError as e:
                rv = f"FAULT({e})"
        good = (wt == exp == arm) and (not rv32_ok or rv == exp)
        ok &= good
        print(f"{'PASS' if good else 'FAIL'} check({a},{b}): wasm={wt} arm={arm} rv32={rv} exp={exp}")
    ok &= rv32_ok
    print("== U64 FUNCCHECK:", "ALL GREEN ==" if ok else "RED (synth#311/#312 class) ==")
    sys.exit(0 if ok else 1)

if __name__ == "__main__":
    main()
