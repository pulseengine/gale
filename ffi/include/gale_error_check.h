/*
 * Copyright (c) 2026 PulseEngine
 * SPDX-License-Identifier: Apache-2.0
 *
 * STPA GAP-6: Compile-time verification that Gale's error codes
 * match Zephyr's minimal libc errno.h values.
 *
 * Include this header in any C shim to get a compile-time check.
 * If Zephyr changes an errno value, the build fails immediately.
 */

#ifndef GALE_ERROR_CHECK_H_
#define GALE_ERROR_CHECK_H_

#include <errno.h>

/* Gale returns negated errno values (e.g., -EINVAL = -22).
 * Verify the base values match Zephyr's definitions. */
_Static_assert(EPERM == 1, "EPERM mismatch: Gale uses -1");
_Static_assert(EAGAIN == 11, "EAGAIN mismatch: Gale uses -11");
_Static_assert(ENOMEM == 12, "ENOMEM mismatch: Gale uses -12");
_Static_assert(EBUSY == 16, "EBUSY mismatch: Gale uses -16");
_Static_assert(EINVAL == 22, "EINVAL mismatch: Gale uses -22");
_Static_assert(EPIPE == 32, "EPIPE mismatch: Gale uses -32");
_Static_assert(ENOMSG == 35, "ENOMSG mismatch: Gale uses -35 (Zephyr minimal libc)");

#endif /* GALE_ERROR_CHECK_H_ */
