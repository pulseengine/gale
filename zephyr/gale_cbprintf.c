/*
 * Copyright (c) 1997-2010, 2012-2015 Wind River Systems, Inc.
 * Copyright (c) 2020 Nordic Semiconductor ASA
 * Copyright (c) 2021 BayLibre, SAS
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale cbprintf — verified format string and buffer validation.
 *
 * This C shim intercepts the safety-critical validation paths in Zephyr's
 * cbprintf subsystem and delegates them to formally verified Rust code.
 *
 * Pattern: Extract → Decide → Apply
 *   Extract: C reads specifier char, width, precision, flags, buffer state
 *   Decide:  Rust validates via gale_cbprintf_validate_* / bounds_check
 *   Apply:   C propagates the error or continues normal processing
 *
 * Verified operations (Verus SMT proofs):
 *   CB1  gale_cbprintf_validate_format_spec — width/prec bounds
 *   CB2  gale_cbprintf_package_bounds_check — package buffer overflow
 *   CB3  gale_cbprintf_output_add          — output length saturation
 *   CB4  gale_cbprintf_validate_specifier  — dangerous specifier rejection
 *   CB5  gale_cbprintf_validate_specifier  — %n always rejected
 *
 * What remains in C:
 *   - Floating-point formatting (cbprintf_complete.c)
 *   - va_list reconstruction (cbprintf_packaged.c)
 *   - String pointer relocation (cbprintf_packaged.c)
 *   - Callback dispatch (cbprintf.c / cbprintf_nano.c)
 *   - SYS_PORT_TRACING_* instrumentation
 */

#include <errno.h>
#include <stdarg.h>
#include <stdint.h>
#include <zephyr/sys/cbprintf.h>
#include <zephyr/sys/__assert.h>
#include <zephyr/logging/log.h>

#include "gale_cbprintf.h"

LOG_MODULE_DECLARE(os, CONFIG_KERNEL_LOG_LEVEL);

/*
 * gale_cbprintf_check_specifier() — validate a conversion specifier char.
 *
 * Called from the format-string scanner before processing each % field.
 *
 * Returns 0 if safe, -EINVAL if the specifier is %n or unrecognised.
 *
 * CB4 + CB5: %n and invalid specifiers are always rejected.
 */
int gale_cbprintf_check_specifier(char specifier_char)
{
	int rc = gale_cbprintf_validate_specifier((uint8_t)specifier_char);

	if (rc != 0) {
		LOG_ERR("cbprintf: rejected specifier '%%%c' (rc=%d)",
			specifier_char, rc);
	}

	return rc;
}

/*
 * gale_cbprintf_check_format_spec() — validate a complete format specifier.
 *
 * Called after parsing width, precision, and flags for a single % field.
 *
 * Returns 0 if valid, -EINVAL if any bound or combination is invalid.
 *
 * CB1: width and precision must fit in [0, INT_MAX].
 * CB4: oversized / invalid specifiers are rejected.
 * CB5: %n is always rejected.
 */
int gale_cbprintf_check_format_spec(char     specifier_char,
				    uint32_t width_value,
				    uint32_t prec_value,
				    bool     flag_dash,
				    bool     flag_zero)
{
	int rc = gale_cbprintf_validate_format_spec(
		(uint8_t)specifier_char,
		width_value,
		prec_value,
		flag_dash ? 1U : 0U,
		flag_zero ? 1U : 0U);

	if (rc != 0) {
		LOG_ERR("cbprintf: invalid format spec '%%%c' w=%u p=%u "
			"flags(dash=%d zero=%d) rc=%d",
			specifier_char, width_value, prec_value,
			(int)flag_dash, (int)flag_zero, rc);
	}

	return rc;
}

/*
 * gale_cbprintf_check_package_write() — pre-write bounds check.
 *
 * Called before writing `size` bytes to the package buffer.
 * Returns 0 on success, -ENOMEM if the write would overflow.
 *
 * CB2: package buffer never overflows.
 */
int gale_cbprintf_check_package_write(size_t pos, size_t capacity, size_t size)
{
	int rc = gale_cbprintf_package_bounds_check(
		(uintptr_t)pos, (uintptr_t)capacity, (uintptr_t)size);

	if (rc != 0) {
		LOG_ERR("cbprintf: package overflow pos=%zu cap=%zu size=%zu",
			pos, capacity, size);
	}

	return rc;
}

/*
 * gale_cbprintf_accumulate_output() — record bytes written to output.
 *
 * Updates *count_inout with the new total.  Returns 0 on success,
 * -EOVERFLOW if the counter saturated (output too long).
 *
 * CB3: output length is tracked accurately and saturates on overflow.
 */
int gale_cbprintf_accumulate_output(size_t *count_inout, size_t n)
{
	if (count_inout == NULL) {
		return -EINVAL;
	}

	uintptr_t new_count = 0;
	int rc = gale_cbprintf_output_add(
		(uintptr_t)*count_inout, (uintptr_t)n, &new_count);

	*count_inout = (size_t)new_count;

	if (rc != 0) {
		LOG_ERR("cbprintf: output length overflow count=%zu n=%zu",
			*count_inout, n);
	}

	return rc;
}
