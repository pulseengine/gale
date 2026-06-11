/* wasm-cross-LTO trampoline for the dissolved z_impl_k_sem_give.
 *
 * The shipped object's body (synth_k_sem_give_body) addresses its pointer
 * argument via the wasm linear-memory base register r11: [r11 + ptr]. With
 * r11 = 0 that is a native dereference, so this thin AAPCS-compatible
 * wrapper zeroes r11 around the call. Validated on NUCLEO-G474RE silicon
 * (engine_control bench; see gale-smart-data NOTES-wasm-cross-lto-spike.md).
 */
	.syntax unified
	.thumb
	.section .text.gale_wasm_sem_tramp,"ax",%progbits
	.global z_impl_k_sem_give
	.thumb_func
z_impl_k_sem_give:
	push	{r11, lr}
	mov.w	r11, #0
	bl	synth_k_sem_give_body
	pop	{r11, pc}
