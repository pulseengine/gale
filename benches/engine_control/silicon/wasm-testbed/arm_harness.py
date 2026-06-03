#!/usr/bin/env python3
"""
ARM-level test harness for synth output: actually executes control_step_decide
(synth's Cortex-M4 Thumb-2 code) under unicorn, with the lookup tables loaded
and fp set to the linear-memory base. Verifies the ARM output's functional
correctness directly (not just the wasm), and pins the suspected coolant-drop
to a concrete got-vs-expected at the instruction level.

Memory model:
  CODE  @ 0x08000000  : control_algo.text.bin
  RAM   @ 0x20000000  : stack (grows down from sp) + linmem
  fp(r11) = 0x20000000 (linmem base); synth reads tables at [fp+0x10000]/[fp+0x10190]
  blob  @ 0x20010000  : [spark int8[20][20] @+0][fuel u16[20][20] @+0x190]
AAPCS: r0=rpm r1=load r2=coolant r3=knock ; return packed (spark<<16|fuel) in r0.
"""
import struct, sys
from unicorn import *
from unicorn.arm_const import *

CODE_BASE = 0x08000000
RAM_BASE  = 0x20000000
RAM_SIZE  = 0x20000          # 128 KB
FP        = RAM_BASE         # linmem base
BLOB_ADDR = RAM_BASE + 0x10000
SP        = RAM_BASE + 0x0FF00
STOP      = 0x08001000       # sentinel return address (in mapped code region)

code = open("control_algo.text.bin","rb").read()
blob = open("tables.blob","rb").read()

def run(rpm, load, coolant, knock):
    mu = Uc(UC_ARCH_ARM, UC_MODE_THUMB | UC_MODE_MCLASS)
    mu.mem_map(CODE_BASE, 0x2000)
    mu.mem_map(RAM_BASE, RAM_SIZE)
    mu.mem_write(CODE_BASE, code)
    mu.mem_write(BLOB_ADDR, blob)
    mu.reg_write(UC_ARM_REG_R0, rpm & 0xffffffff)
    mu.reg_write(UC_ARM_REG_R1, load & 0xffffffff)
    mu.reg_write(UC_ARM_REG_R2, coolant & 0xffffffff)
    mu.reg_write(UC_ARM_REG_R3, knock & 0xffffffff)
    mu.reg_write(UC_ARM_REG_SP, SP)
    mu.reg_write(UC_ARM_REG_R11, FP)
    mu.reg_write(UC_ARM_REG_LR, STOP | 1)   # thumb bit
    mu.emu_start(CODE_BASE | 1, STOP, count=0)
    r0 = mu.reg_read(UC_ARM_REG_R0)
    return r0 & 0xffffffff

# (rpm, load, coolant, knock) -> expected packed (from native reference)
VEC = [
    (3000, 50, 90, 0, 0x002108FC),   # hot: enrich 0
    (3000, 50, 40, 0, 0x00210A55),   # mid coolant: enrich 150 -> fuel 2645 (DISCRIMINATOR)
    (3000, 50, 0,  0, 0x00210BAE),   # cold: enrich 300
    (6000, 80, 40, 3, 0x002208E5),
]
bad = 0
for rpm, load, cool, knock, exp in VEC:
    try:
        got = run(rpm, load, cool, knock)
    except UcError as e:
        print(f"({rpm},{load},{cool},{knock}) -> UC ERROR {e}"); bad += 1; continue
    gs, gf = (got>>16)&0xffff, got&0xffff
    es, ef = (exp>>16)&0xffff, exp&0xffff
    ok = (got == exp)
    print(f"[{'OK ' if ok else 'BAD'}] ({rpm},{load},{cool},{knock}) "
          f"got=0x{got:08x}(s{gs}/f{gf}) exp=0x{exp:08x}(s{es}/f{ef})")
    if not ok: bad += 1
print("ALL PASS" if bad == 0 else f"{bad} MISMATCH")
sys.exit(1 if bad else 0)
