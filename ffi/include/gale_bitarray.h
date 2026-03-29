/*
 * Gale Bitarray FFI — verified bounds validation for bitarray.c.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#ifndef GALE_BITARRAY_H
#define GALE_BITARRAY_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ---- Allocation validation ---- */

int32_t gale_bitarray_alloc_validate(
    uint32_t num_bits, uint32_t offset, uint32_t alloc_nbits);

/* ---- Region validation ---- */

int32_t gale_bitarray_region_check(
    uint32_t num_bits, uint32_t offset, uint32_t region_nbits);

/* ---- Single bit validation ---- */

int32_t gale_bitarray_set_bit_validate(
    uint32_t num_bits, uint32_t bit);

/* ---- Bundle index computation ---- */

struct gale_bitarray_bundle_index {
    uint32_t bundle_index;  /* bit / 32 */
    uint32_t bit_offset;    /* bit % 32 */
};

struct gale_bitarray_bundle_index gale_bitarray_bundle_index(
    uint32_t bit);

#ifdef __cplusplus
}
#endif

#endif /* GALE_BITARRAY_H */
