/*
 * Gale Red-Black Tree FFI — verified color invariant validation for rb.c.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#ifndef GALE_RB_H
#define GALE_RB_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ---- Insert color validation ---- */

int32_t gale_rb_validate_insert(
    uint8_t is_black, uint8_t parent_is_black,
    uint8_t has_left, uint8_t has_right);

/* ---- Post-rotation color validation ---- */

int32_t gale_rb_validate_color_after_rotation(
    uint8_t node_color, uint8_t child_color);

#ifdef __cplusplus
}
#endif

#endif /* GALE_RB_H */
