/*
 * Copyright (c) 2018 Intel Corporation
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale red-black tree — verified color invariant validation for rb.c.
 *
 * This is NOT a replacement for lib/utils/rb.c. It provides assertion
 * wrappers that call Rust-verified invariant checks at key points in
 * the tree operations.
 *
 * rb.c is deeply pointer-based with intrusive node structures. Rather
 * than replacing it, this shim adds verified assertions:
 *
 *   - After insert: validate no red-red violation exists
 *   - After rotation: validate the color swap is correct
 *
 * Pattern: Assert-after-operation
 *   The upstream rb.c performs the operation, then this module's
 *   assertion helpers verify the invariant via Rust FFI.
 *
 * Verified operations (Verus proofs):
 *   gale_rb_validate_insert              — RBT1 (no consecutive red nodes)
 *   gale_rb_validate_color_after_rotation — RBT2 (rotation color swap)
 */

#include <zephyr/kernel.h>
#include <zephyr/sys/rb.h>
#include <zephyr/sys/__assert.h>

#include "gale_rb.h"

/*
 * Validate that a node-parent pair does not violate the red-red property.
 *
 * This wraps gale_rb_validate_insert to provide a convenient C API
 * that extracts color information from rbnode pointers.
 *
 * Can be called after rb_insert() to assert the tree is well-formed,
 * or at any traversal point to audit the tree.
 *
 * Arguments:
 *   node:   the rbnode to check (must not be NULL)
 *   parent: the parent rbnode (may be NULL for root)
 *
 * Returns:
 *   0       — no red-red violation
 *   -EINVAL — red-red violation detected
 */
int gale_rb_assert_no_red_red(struct rbnode *node, struct rbnode *parent)
{
	uint8_t is_black;
	uint8_t parent_is_black;
	uint8_t has_left;
	uint8_t has_right;

	__ASSERT_NO_MSG(node != NULL);

	is_black = z_rb_is_black(node) ? 1U : 0U;

	if (parent == NULL) {
		/* Root node — no parent to compare against, always valid */
		parent_is_black = 1U;
	} else {
		parent_is_black = z_rb_is_black(parent) ? 1U : 0U;
	}

	has_left = (z_rb_child(node, 0U) != NULL) ? 1U : 0U;
	has_right = (z_rb_child(node, 1U) != NULL) ? 1U : 0U;

	return (int)gale_rb_validate_insert(
		is_black, parent_is_black, has_left, has_right);
}

/*
 * Validate color assignment after a rotation.
 *
 * After a rotation in fix_extra_red, the promoted node must be BLACK
 * and the demoted node must be RED. This function verifies that
 * invariant via the Rust FFI.
 *
 * Arguments:
 *   promoted: the node that was rotated up (must not be NULL)
 *   demoted:  the node that was rotated down (must not be NULL)
 *
 * Returns:
 *   0       — correct color assignment (promoted=BLACK, demoted=RED)
 *   -EINVAL — incorrect color assignment
 */
int gale_rb_assert_rotation_colors(struct rbnode *promoted,
				   struct rbnode *demoted)
{
	uint8_t promoted_color;
	uint8_t demoted_color;

	__ASSERT_NO_MSG(promoted != NULL);
	__ASSERT_NO_MSG(demoted != NULL);

	/* Extract colors: 1 = BLACK, 0 = RED (matching rb.c encoding) */
	promoted_color = z_rb_is_black(promoted) ? 1U : 0U;
	demoted_color = z_rb_is_black(demoted) ? 1U : 0U;

	return (int)gale_rb_validate_color_after_rotation(
		promoted_color, demoted_color);
}

/*
 * Full tree audit: walk a subtree and assert no red-red violations.
 *
 * This is a debug/test utility that recursively checks every
 * parent-child pair in the tree. Expensive (O(n)) but useful for
 * post-operation validation in test builds.
 *
 * Arguments:
 *   node:   current node (may be NULL for leaves)
 *   parent: parent of current node (NULL for root)
 *
 * Returns:
 *   0       — entire subtree is valid
 *   -EINVAL — at least one red-red violation found
 */
int gale_rb_audit_subtree(struct rbnode *node, struct rbnode *parent)
{
	int rc;

	if (node == NULL) {
		return 0;
	}

	/* Check this node against its parent */
	rc = gale_rb_assert_no_red_red(node, parent);
	if (rc != 0) {
		return rc;
	}

	/* Recurse into children */
	rc = gale_rb_audit_subtree(z_rb_child(node, 0U), node);
	if (rc != 0) {
		return rc;
	}

	return gale_rb_audit_subtree(z_rb_child(node, 1U), node);
}
