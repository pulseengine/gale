/*
 * Gale Work FFI — verified work item state machine.
 *
 * Phase 2: Decision struct pattern (Extract->Decide->Apply).
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#ifndef GALE_WORK_H
#define GALE_WORK_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ---- Phase 2: Full Decision API ---- */

struct gale_work_submit_decision {
    uint8_t action;     /* 0=QUEUE, 1=REQUEUE, 2=ALREADY, 3=REJECT */
    uint8_t new_flags;
    int32_t ret;        /* 1=queued, 2=re-queued, 0=already, -EBUSY=rejected */
};

#define GALE_WORK_SUBMIT_QUEUE    0
#define GALE_WORK_SUBMIT_REQUEUE  1
#define GALE_WORK_SUBMIT_ALREADY  2
#define GALE_WORK_SUBMIT_REJECT   3

/**
 * Decide a work submit operation.
 *
 * C extracts work->flags under spinlock, passes decomposed state.
 * Rust decides the action; C applies it.
 *
 * @param flags      Current work item flags (uint32_t cast to u8).
 * @param is_queued  1 if K_WORK_QUEUED_BIT is set, 0 otherwise.
 * @param is_running 1 if K_WORK_RUNNING_BIT is set, 0 otherwise.
 *
 * @return Decision struct with action, new_flags, and return code.
 */
struct gale_work_submit_decision gale_k_work_submit_decide(
    uint8_t flags, uint8_t is_queued, uint8_t is_running);

struct gale_work_cancel_decision {
    uint8_t action;     /* 0=IDLE, 1=DEQUEUE, 2=SET_CANCELING */
    uint8_t new_flags;
    uint8_t busy;       /* busy status after cancel */
};

#define GALE_WORK_CANCEL_IDLE      0
#define GALE_WORK_CANCEL_DEQUEUE   1
#define GALE_WORK_CANCEL_CANCELING 2

/**
 * Decide a work cancel operation.
 *
 * C extracts work->flags under spinlock, passes decomposed state.
 * Rust decides the action; C applies it (dequeue, set flags).
 *
 * @param flags      Current work item flags.
 * @param is_queued  1 if K_WORK_QUEUED_BIT is set, 0 otherwise.
 * @param is_running 1 if K_WORK_RUNNING_BIT is set, 0 otherwise.
 *
 * @return Decision struct with action, new_flags, and busy status.
 */
struct gale_work_cancel_decision gale_k_work_cancel_decide(
    uint8_t flags, uint8_t is_queued, uint8_t is_running);

/* ---- Legacy validate API (backward compat) ---- */

int32_t gale_work_submit_validate(uint8_t flags, uint8_t *new_flags);

int32_t gale_work_cancel_validate(uint8_t flags,
                                   uint8_t *new_flags,
                                   uint8_t *busy);

#ifdef __cplusplus
}
#endif

#endif /* GALE_WORK_H */
