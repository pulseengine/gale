/* wasm-cross-LTO trampoline for the dissolved z_impl_k_msgq_put.
 *
 * The shipped object's body (synth_k_msgq_put_body) addresses its k_msgq *
 * argument's fields via the wasm linear-memory base register r11: [r11 + ptr].
 * With r11 = 0 that is a native dereference, so this thin AAPCS-compatible
 * wrapper zeroes r11 around the call (the --native-pointer-abi convention).
 * Arguments pass straight through: r0 = msgq*, r1 = data*, r2:r3 = the 8-byte
 * k_timeout_t (== int64_t ticks; the shim's third parameter). Validated on
 * NUCLEO-G474RE silicon: k_msgq_put = 673 cyc, SELFCHECK rc=0 val=0xABCD on
 * synth 0.11.48 (post synth#359 .bss-sizing fix). See gale-smart-data
 * NOTES-wasm-cross-lto-spike.md + msgq-microbench/.
 */
	.syntax unified
	.thumb
	.section .text.gale_wasm_msgq_tramp,"ax",%progbits
	.global z_impl_k_msgq_put
	.thumb_func
z_impl_k_msgq_put:
	push	{r11, lr}
	mov.w	r11, #0
	bl	synth_k_msgq_put_body
	pop	{r11, pc}
