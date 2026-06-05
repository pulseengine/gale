	.syntax unified
	.thumb
	.section .text.cs_tramp,"ax",%progbits
	.global control_step_decide
	.thumb_func
control_step_decide:
	push {r11, lr}
	movw r11, :lower16:wasm_linmem
	movt r11, :upper16:wasm_linmem
	movw r12, :lower16:synth_cs_body
	movt r12, :upper16:synth_cs_body
	orr  r12, r12, #1
	blx  r12
	pop  {r11, pc}
