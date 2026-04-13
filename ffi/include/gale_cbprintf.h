/*
 * Gale cbprintf FFI — verified format string and buffer validation.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#ifndef GALE_CBPRINTF_H
#define GALE_CBPRINTF_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/*
 * Conversion specifier codes passed to gale_cbprintf_validate_specifier.
 * Mirror the ASCII values of the printf conversion characters.
 */

/**
 * Validate a single printf conversion specifier character.
 *
 * CB4 + CB5: %n and unknown specifiers are always rejected.
 *
 * @param specifier_char  ASCII character of the specifier (e.g. 'd', 's', 'n').
 *
 * @return 0 on success, -EINVAL if the specifier is %n or unrecognised.
 */
int32_t gale_cbprintf_validate_specifier(uint8_t specifier_char);

/**
 * Validate format specifier bounds (width, precision, flags).
 *
 * CB1 + CB4: ensures width and precision are within [0, INT_MAX] and
 * that the flag combination is consistent (e.g. '-' overrides '0').
 *
 * @param specifier_char  ASCII character of the specifier.
 * @param width_value     Width field value (0 if not present).
 * @param prec_value      Precision field value (0 if not present).
 * @param flag_dash       1 if '-' flag present, 0 otherwise.
 * @param flag_zero       1 if '0' flag present, 0 otherwise.
 *
 * @return 0 on success, -EINVAL if any bound or combination is invalid.
 */
int32_t gale_cbprintf_validate_format_spec(uint8_t  specifier_char,
                                            uint32_t width_value,
                                            uint32_t prec_value,
                                            uint8_t  flag_dash,
                                            uint8_t  flag_zero);

/**
 * Check whether writing `size` bytes into the package buffer would overflow.
 *
 * CB2: package buffer never overflows.
 *
 * @param pos       Current buffer position (bytes from start).
 * @param capacity  Total buffer capacity in bytes.
 * @param size      Number of bytes about to be written.
 *
 * @return 0 if write is safe; -ENOMEM if it would overflow.
 */
int32_t gale_cbprintf_package_bounds_check(uintptr_t pos,
                                            uintptr_t capacity,
                                            uintptr_t size);

/**
 * Accumulate output bytes, detecting overflow in the byte counter.
 *
 * CB3: output length is tracked accurately and saturates rather than
 * wrapping on overflow.
 *
 * @param count     Current byte count.
 * @param n         Bytes being added.
 * @param out_count Updated byte count (saturated at UINTPTR_MAX/2 on overflow).
 *
 * @return 0 on success; -EOVERFLOW if saturation occurred.
 */
int32_t gale_cbprintf_output_add(uintptr_t  count,
                                  uintptr_t  n,
                                  uintptr_t *out_count);

#ifdef __cplusplus
}
#endif

#endif /* GALE_CBPRINTF_H */
