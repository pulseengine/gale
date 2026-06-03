/* wasm-cross-LTO trampoline for flight_control: the bench's filter_step /
 * controller_step are dissolved (clang->wasm-ld->loom->synth) into
 * synth_filter_step / synth_controller_step, which address their pointer
 * args via the wasm linear-memory base register r11. Zephyr/the bench call
 * with r11 as an ordinary frame pointer, so these thin wrappers set r11=0
 * ([r11+ptr] == [ptr], native deref), call the synth body, and restore. */
	.syntax unified
	.thumb
	.section .text.gale_fc_tramp,"ax",%progbits

	.global filter_step
	.thumb_func
filter_step:
	push	{r11, lr}
	mov.w	r11, #0
	bl	synth_filter_step
	pop	{r11, pc}

	.global controller_step
	.thumb_func
controller_step:
	push	{r11, lr}
	mov.w	r11, #0
	bl	synth_controller_step
	pop	{r11, pc}
