.section .text
.global rv_cs_call
rv_cs_call:                  # a0=rpm a1=load a2=cool a3=knock -> a0 (synth control_step via s11 table base)
    addi sp,sp,-16
    sw ra,0(sp)
    sw s11,4(sp)
    la s11, gale_tables
    lui t0, 0x10
    sub s11, s11, t0         # s11 = &gale_tables - 0x10000  (so [s11+0x10000+idx] hits the tables)
    call control_step_decide
    lw s11,4(sp)
    lw ra,0(sp)
    addi sp,sp,16
    ret
