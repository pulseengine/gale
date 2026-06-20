#!/usr/bin/env python3
"""
ARM (Cortex-M4 Thumb-2) functional regression lane — the ARM analogue of
arch/riscv/run_rv_funccheck.sh. Executes synth's ARM output for the pure-register
leaves (filter_axis, controller_step) under unicorn across the same wasmtime-
verified edge vectors, so a cross-target codegen change (e.g. const-CSE applied
to the ARM pool) that miscompiles only on certain inputs is caught. control_step
on ARM is covered by arm_harness.py (it needs the table blob + fp base).

These two leaves have 0 memory ops, so no fp/linmem/tables setup is needed.
ABI NOTE: synth's ARM output passes args in REGISTERS r0..r7 (the RISC-V a0..a7
style), NOT AAPCS (r0-r3 + stack for arg5+). Verified by disasm: a 7-arg function
pushes {r4-r8,lr} then reads args 5-7 from the saved r4/r5/r6 slots — i.e. it
received them in r4-r6. So a >4-arg synth function called from a stock-AAPCS C
caller would get garbage for args 5+; the wasm-cross-LTO caller/trampoline must
match synth's register convention. We mirror it: all args go in r0.. sequentially
(controller_step's 7 fit r0-r6). Exit 0 = all PASS.
"""
import struct, subprocess, sys, tempfile, os
from unicorn import *
from unicorn.arm_const import *

CLANG="/opt/homebrew/opt/llvm/bin/clang"; OBJCOPY="/opt/homebrew/opt/llvm/bin/llvm-objcopy"
CODE=0x08000000; RAM=0x20000000; SP=RAM+0x0FF00; STOP=0x08002000
OPT="/tmp/opt3"   # reuse the dissolved loom modules

def text_bin(loom):
    """dissolve already done; synth compile -> cortex-m4f .o -> raw .text bytes."""
    o=tempfile.mktemp(suffix=".o"); b=tempfile.mktemp(suffix=".bin")
    r=subprocess.run(["synth","compile",loom,"--target","cortex-m4f","--all-exports",
                      "--relocatable","-o",o],capture_output=True,text=True)
    if r.returncode!=0: raise RuntimeError("synth compile failed: "+r.stderr[:200])
    subprocess.run([OBJCOPY,"-O","binary","--only-section=.text",o,b],check=True)
    data=open(b,"rb").read(); os.remove(o); os.remove(b); return data

REGS=[UC_ARM_REG_R0,UC_ARM_REG_R1,UC_ARM_REG_R2,UC_ARM_REG_R3,
      UC_ARM_REG_R4,UC_ARM_REG_R5,UC_ARM_REG_R6,UC_ARM_REG_R7]
def run(code, args):
    mu=Uc(UC_ARCH_ARM, UC_MODE_THUMB|UC_MODE_MCLASS)
    mu.mem_map(CODE,0x4000); mu.mem_map(RAM,0x20000)
    mu.mem_write(CODE,code)
    for i,a in enumerate(args):           # synth ABI: args in r0..r7 (not AAPCS stack)
        mu.reg_write(REGS[i], a & 0xffffffff)
    mu.reg_write(UC_ARM_REG_SP, SP)
    mu.reg_write(UC_ARM_REG_LR, STOP|1)
    mu.emu_start(CODE|1, STOP, count=0)
    return mu.reg_read(UC_ARM_REG_R0) & 0xffffffff

U=lambda v: v & 0xffffffff
TESTS={
 "filter_axis":(f"{OPT}/filter.loom.wasm",[
   ("(0,0,0)",[0,0,0],0),
   ("(1000,100,500)",[1000,100,500],1088),
   ("(-2000,50,-300)",[U(-2000),50,U(-300)],U(-1917))]),
 "controller_step":(f"{OPT}/controller.loom.wasm",[
   ("(0..0)",[0,0,0,0,0,0,0],0),
   ("(6400..5)",[6400,0,U(-12800),0,3200,0,5],97419164),
   ("(satclamp)",[99999,99999,U(-99999),U(-99999),99999,U(-99999),255],0xFF817F81)]),
}
bad=0
for fn,(loom,vecs) in TESTS.items():
    code=text_bin(loom)
    for name,args,exp in vecs:
        got=run(code,args)
        ok = got==U(exp)
        print(f"{'PASS' if ok else 'FAIL'} {fn}{name} got={got} exp={U(exp)}")
        if not ok: bad=1
print("== ARM FUNCCHECK: %s ==" % ("ALL GREEN" if not bad else "FAILURES (synth ARM miscompile)"))
sys.exit(bad)
