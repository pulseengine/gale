	.syntax unified
	.thumb
	.section .text.ctl_tramp,"ax",%progbits
	.global controller_step_decide
	.thumb_func
controller_step_decide:
	push {r3, r4, r5, r6, r7, lr}   @ 24B, 8-byte aligned; r3(arg4) preserved untouched
	ldr  r4, [sp, #24]              @ arg5 = yaw_mdeg
	ldr  r5, [sp, #28]              @ arg6 = yaw_rate
	ldr  r6, [sp, #32]             @ arg7 = updates
	movw r7, :lower16:synth_ctl_body
	movt r7, :upper16:synth_ctl_body
	orr  r7, r7, #1                 @ thumb bit (synth#170: no $t)
	blx  r7
	pop  {r3, r4, r5, r6, r7, pc}
