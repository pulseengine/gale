/*
 * Copyright (c) 2021 Intel Corporation
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale bitarray — verified bounds validation for bitarray.c.
 *
 * This is NOT a replacement for lib/utils/bitarray.c. It wraps key
 * bitarray functions with Rust-verified validation calls that check
 * bounds and overflow before the upstream code executes.
 *
 * Pattern: Validate -> Delegate
 *   Validate: Rust checks bounds / overflow via gale_bitarray_*
 *   Delegate: If validation passes, call the original bitarray function
 *
 * The upstream bitarray.c is still compiled and linked. This module
 * provides alternative entry points with pre-validated parameters.
 *
 * Verified operations (Verus proofs):
 *   gale_bitarray_set_bit_validate   — BA1 (bit bounds)
 *   gale_bitarray_alloc_validate     — BA2 (region bounds), BA4 (no overflow)
 *   gale_bitarray_region_check       — BA2 (region bounds), BA4 (no overflow)
 *   gale_bitarray_bundle_index       — BA3 (bundle index < num_bundles)
 */

#include <zephyr/sys/bitarray.h>
#include <zephyr/sys/__assert.h>
#include <errno.h>

#include "gale_bitarray.h"

/*
 * Validated set_bit — calls Rust validation before the operation.
 *
 * The upstream sys_bitarray_set_bit already checks bit >= num_bits,
 * but the Rust validation provides formally verified bounds checking
 * with overflow protection that the C code lacks.
 */
int gale_validated_set_bit(sys_bitarray_t *bitarray, size_t bit)
{
	int32_t rc;

	__ASSERT_NO_MSG(bitarray != NULL);

	/* Validate: Rust formally verified bounds check */
	rc = gale_bitarray_set_bit_validate(
		(uint32_t)bitarray->num_bits, (uint32_t)bit);
	if (rc != 0) {
		return -EINVAL;
	}

	/* Delegate: upstream implementation (already validated) */
	return sys_bitarray_set_bit(bitarray, bit);
}

/*
 * Validated clear_bit — calls Rust validation before the operation.
 *
 * Uses the same set_bit_validate since the bounds check is identical.
 */
int gale_validated_clear_bit(sys_bitarray_t *bitarray, size_t bit)
{
	int32_t rc;

	__ASSERT_NO_MSG(bitarray != NULL);

	/* Validate: Rust formally verified bounds check */
	rc = gale_bitarray_set_bit_validate(
		(uint32_t)bitarray->num_bits, (uint32_t)bit);
	if (rc != 0) {
		return -EINVAL;
	}

	/* Delegate: upstream implementation */
	return sys_bitarray_clear_bit(bitarray, bit);
}

/*
 * Validated test_bit — calls Rust validation before the operation.
 */
int gale_validated_test_bit(sys_bitarray_t *bitarray, size_t bit, int *val)
{
	int32_t rc;

	__ASSERT_NO_MSG(bitarray != NULL);

	/* Validate: Rust formally verified bounds check */
	rc = gale_bitarray_set_bit_validate(
		(uint32_t)bitarray->num_bits, (uint32_t)bit);
	if (rc != 0) {
		return -EINVAL;
	}

	/* Delegate: upstream implementation */
	return sys_bitarray_test_bit(bitarray, bit, val);
}

/*
 * Validated alloc — calls Rust validation before attempting allocation.
 *
 * The Rust validation uses checked_add to detect overflow in
 * offset + alloc_nbits, which the upstream C code computes without
 * overflow protection.
 */
int gale_validated_alloc(sys_bitarray_t *bitarray, size_t num_bits,
			 size_t *offset)
{
	__ASSERT_NO_MSG(bitarray != NULL);

	/* Validate: Rust formally verified bounds + overflow check */
	int32_t rc = gale_bitarray_alloc_validate(
		(uint32_t)bitarray->num_bits, 0U, (uint32_t)num_bits);
	if (rc != 0) {
		return -EINVAL;
	}

	/* Delegate: upstream implementation */
	return sys_bitarray_alloc(bitarray, num_bits, offset);
}

/*
 * Validated set_region — calls Rust region check before the operation.
 *
 * The Rust validation uses checked_add to detect overflow in
 * offset + region_nbits.
 */
int gale_validated_set_region(sys_bitarray_t *bitarray, size_t num_bits,
			      size_t offset)
{
	__ASSERT_NO_MSG(bitarray != NULL);

	/* Validate: Rust formally verified region bounds check */
	int32_t rc = gale_bitarray_region_check(
		(uint32_t)bitarray->num_bits, (uint32_t)offset,
		(uint32_t)num_bits);
	if (rc != 0) {
		return -EINVAL;
	}

	/* Delegate: upstream implementation */
	return sys_bitarray_set_region(bitarray, num_bits, offset);
}

/*
 * Validated clear_region — calls Rust region check before the operation.
 */
int gale_validated_clear_region(sys_bitarray_t *bitarray, size_t num_bits,
				size_t offset)
{
	__ASSERT_NO_MSG(bitarray != NULL);

	/* Validate: Rust formally verified region bounds check */
	int32_t rc = gale_bitarray_region_check(
		(uint32_t)bitarray->num_bits, (uint32_t)offset,
		(uint32_t)num_bits);
	if (rc != 0) {
		return -EINVAL;
	}

	/* Delegate: upstream implementation */
	return sys_bitarray_clear_region(bitarray, num_bits, offset);
}

/*
 * Validated free — calls Rust region check before freeing.
 *
 * sys_bitarray_free checks both bounds AND whether the bits are
 * actually allocated (set). We validate bounds first via Rust.
 */
int gale_validated_free(sys_bitarray_t *bitarray, size_t num_bits,
			size_t offset)
{
	__ASSERT_NO_MSG(bitarray != NULL);

	/* Validate: Rust formally verified region bounds check */
	int32_t rc = gale_bitarray_region_check(
		(uint32_t)bitarray->num_bits, (uint32_t)offset,
		(uint32_t)num_bits);
	if (rc != 0) {
		return -EINVAL;
	}

	/* Delegate: upstream implementation */
	return sys_bitarray_free(bitarray, num_bits, offset);
}
