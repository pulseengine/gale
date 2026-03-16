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

/* Recovery action codes returned by gale_fatal_classify */
#define GALE_FATAL_ABORT_THREAD  0
#define GALE_FATAL_HALT          1
#define GALE_FATAL_IGNORE        2

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

#ifdef __cplusplus
}
#endif

#endif /* GALE_FATAL_H */
