/*
 * Gale Fatal FFI — verified fatal error classification.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#ifndef GALE_FATAL_H
#define GALE_FATAL_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Recovery action codes */
#define GALE_FATAL_ACTION_ABORT_THREAD  0
#define GALE_FATAL_ACTION_HALT          1
#define GALE_FATAL_ACTION_IGNORE        2

/* Legacy defines for backward compatibility */
#define GALE_FATAL_ABORT_THREAD  GALE_FATAL_ACTION_ABORT_THREAD
#define GALE_FATAL_HALT          GALE_FATAL_ACTION_HALT
#define GALE_FATAL_IGNORE        GALE_FATAL_ACTION_IGNORE

/**
 * Classify a fatal error: determine recovery action.
 *
 * @param reason     Error reason code (0=CPU_EXCEPTION, 1=SPURIOUS_IRQ,
 *                   2=STACK_CHECK_FAIL, 3=KERNEL_OOPS, 4=KERNEL_PANIC).
 * @param is_isr     1 if in ISR context, 0 if in thread context.
 * @param test_mode  1 if CONFIG_TEST, 0 for production.
 *
 * @return 0 (AbortThread), 1 (Halt), 2 (Ignore), -EINVAL on bad reason.
 */
int32_t gale_fatal_classify(uint32_t reason,
                             uint32_t is_isr,
                             uint32_t test_mode);

/* ---- Phase 2: Full Decision API ---- */

struct gale_fatal_decision {
    uint8_t action;     /* 0=ABORT_THREAD, 1=HALT, 2=IGNORE */
    int32_t ret;        /* 0 on success, -EINVAL for unknown reason */
};

/**
 * Full decision for fatal error classification.
 *
 * Returns the 8-byte gale_fatal_decision packed into uint64_t so AAPCS
 * uses r0/r1 instead of sret — required for the LLVM cross-language
 * inliner to inline this call (see gale issue #10). Caller decodes
 * via the union helper below.
 *
 * @param reason     Error reason code (0-4).
 * @param is_isr     1 if in ISR context, 0 if in thread context.
 * @param test_mode  1 if CONFIG_TEST, 0 for production.
 *
 * @return 8-byte decision packed into u64 (action low byte, ret high i32).
 *
 * Verified: FT1 (reason mapping), FT2 (panic halts), FT3 (recovery).
 */
uint64_t gale_k_fatal_decide(uint32_t reason,
                              uint32_t is_isr,
                              uint32_t test_mode);

/* Helper union: cast the u64 return back to the typed struct. */
union gale_fatal_decision_u {
    uint64_t raw;
    struct gale_fatal_decision dec;
};

#ifdef __cplusplus
}
#endif

#endif /* GALE_FATAL_H */
