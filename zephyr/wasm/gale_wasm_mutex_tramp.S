/* wasm-cross-LTO trampoline for the dissolved z_impl_k_mutex_unlock.
 *
 * The shipped object's body (synth_k_mutex_unlock_body) addresses its k_mutex *
 * argument's fields via the wasm linear-memory base register r11: [r11 + ptr].
 * With r11 = 0 that is a native dereference, so this thin AAPCS-compatible
 * wrapper zeroes r11 around the call (the --native-pointer-abi convention).
 * Validated on NUCLEO-G474RE silicon: k_mutex_unlock = 501 cyc, SELFCHECK
 * rc=0 owner=0 (no deadlock) on synth 0.11.41 (post synth#331 fix). See
 * gale-smart-data NOTES-wasm-cross-lto-spike.md.
 */
	.syntax unified
	.thumb
	.section .text.gale_wasm_mutex_tramp,"ax",%progbits
	.global z_impl_k_mutex_unlock
	.thumb_func
z_impl_k_mutex_unlock:
	push	{r11, lr}
	mov.w	r11, #0
	bl	synth_k_mutex_unlock_body
	pop	{r11, pc}
