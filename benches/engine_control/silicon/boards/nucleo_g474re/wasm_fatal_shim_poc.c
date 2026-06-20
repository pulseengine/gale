/*
 * Minimal wasm-side host of k_sys_fatal_error_handler (the dissolved fatal-policy path).
 *
 * The 5th and last of the maintainer's u64-shaped clean decide set
 * (sem_give, sem_take, pipe_write, pipe_read, fatal). Closes the u64-clean
 * partition. Same construction as sem_give_shim.c: kernel APIs are externs
 * (wasm imports), the verified Rust decide (gale_k_fatal_decide) rides the
 * bundle so loom inlines the seam. Pure scalar-in/u64-out decide → sem-class,
 * NOT --native-pointer-abi (the esf pointer is not dereferenced by the policy).
 *
 * Zephyr hook: k_sys_fatal_error_handler(reason, esf) is __weak — gale overrides
 * it. The decide classifies reason → ABORT_THREAD (return, z_fatal_error aborts
 * the faulting thread) / HALT (k_fatal_halt, no return) / IGNORE (return, continue).
 */

#include <stdint.h>

struct arch_esf; /* opaque — the policy never derefs it */

/* Kernel API externs → wasm imports → native bl after synth-emit. */
extern uint32_t gale_w_in_isr(void);            /* arch_is_in_isr(), out-of-line */
extern void     k_fatal_halt(unsigned int reason); /* FUNC_NORETURN */

/* The verified Rust decide — same wasm module after merge; loom inlines it. */
extern uint64_t gale_k_fatal_decide(uint32_t reason, uint32_t is_isr, uint32_t test_mode);

/* AAPCS-packed decision: matches #[repr(C)] GaleFatalDecision (8 bytes). */
union gale_fatal_decision_u {
    uint64_t raw;
    struct {
        uint8_t  action;
        uint8_t  pad[3];
        int32_t  ret;
    } dec;
};

#define GALE_FATAL_ACTION_ABORT_THREAD 0
#define GALE_FATAL_ACTION_HALT         1
#define GALE_FATAL_ACTION_IGNORE       2

/* The dissolved fatal-policy hook. THIS IS THE FFI SEAM. */
void k_sys_fatal_error_handler(unsigned int reason, const struct arch_esf *esf)
{
    (void)esf; /* policy is reason-driven; arch layer owns the esf */

    union gale_fatal_decision_u du;
    du.raw = gale_k_fatal_decide(reason, gale_w_in_isr() ? 1U : 0U, 0U);

    switch (du.dec.action) {
    case GALE_FATAL_ACTION_HALT:
        k_fatal_halt(reason); /* never returns */
        break;
    case GALE_FATAL_ACTION_ABORT_THREAD:
    case GALE_FATAL_ACTION_IGNORE:
    default:
        /* return to z_fatal_error: ABORT_THREAD → it aborts the faulting
         * thread; IGNORE → execution continues. */
        break;
    }
}
