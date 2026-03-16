/*
 * Gale Work FFI — verified work item state machine.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#ifndef GALE_WORK_H
#define GALE_WORK_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Validate a work submit operation.
 *
 * @param flags      Current work item flags.
 * @param new_flags  Output: updated flags.
 *
 * @return 1 (newly queued), 2 (re-queued running), 0 (already queued),
 *         -EBUSY (canceling), -EINVAL (null pointer).
 */
int32_t gale_work_submit_validate(uint8_t flags, uint8_t *new_flags);

/**
 * Validate a work cancel operation.
 *
 * @param flags      Current work item flags.
 * @param new_flags  Output: updated flags.
 * @param busy       Output: busy status after cancel.
 *
 * @return 0 on success, -EINVAL on null pointer.
 */
int32_t gale_work_cancel_validate(uint8_t flags,
                                   uint8_t *new_flags,
                                   uint8_t *busy);

#ifdef __cplusplus
}
#endif

#endif /* GALE_WORK_H */
