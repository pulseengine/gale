	.syntax unified
	.thumb
	.section .text.ff_tramp,"ax",%progbits
	.global synth_flat_flight_buf
	.thumb_func
synth_flat_flight_buf:            @ (uint32_t st_off=r0, uint32_t s_off=r1)
	push {r11, lr}
	movw r11, :lower16:wasm_linmem
	movt r11, :upper16:wasm_linmem
	movw r2, :lower16:synth_flat_flight_body
	movt r2, :upper16:synth_flat_flight_body
	orr  r2, r2, #1               @ thumb bit (synth#170)
	blx  r2
	pop  {r11, pc}
