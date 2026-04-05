/*
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale condvar — verified wait-queue decision functions for k_condvar.
 *
 * These three functions replace the action-decision logic in
 * kernel/condvar.c using the Extract→Decide→Apply pattern.
 * The wait queue side effects (z_unpend_first_thread, z_ready_thread,
 * z_pend_curr, mutex lock/unlock) remain in C.
 *
 * Verified operations (Verus + Rocq proofs):
 *   gale_k_condvar_signal_decide    — C2 (wake one), C3 (no-op empty)
 *   gale_k_condvar_broadcast_decide — C4 (wake all), C5 (0 when empty), C8
 *   gale_k_condvar_wait_decide      — C6 (pend or EAGAIN)
 */

#ifndef GALE_CONDVAR_H_
#define GALE_CONDVAR_H_

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ---- Signal decision ---- */

struct gale_condvar_signal_decision {
    uint8_t action; /* 0 = NOOP, 1 = WAKE_ONE */
};

#define GALE_CONDVAR_SIGNAL_NOOP     0
#define GALE_CONDVAR_SIGNAL_WAKE_ONE 1

/**
 * Decide action for k_condvar_signal.
 *
 * @param has_waiter  Non-zero if the condvar wait queue is non-empty.
 * @return            NOOP or WAKE_ONE.
 *
 * Verified: C2 (at most one waiter woken), C3 (no-op when empty), C7.
 */
struct gale_condvar_signal_decision gale_k_condvar_signal_decide(uint32_t has_waiter);

/* ---- Broadcast decision ---- */

struct gale_condvar_broadcast_decision {
    uint32_t woken; /* number of threads to wake */
};

/**
 * Decide action for k_condvar_broadcast.
 *
 * @param num_waiters  Current wait queue length.
 * @return             Number of threads to wake (0 if empty).
 *
 * Verified: C4 (all waiters woken), C5 (0 when empty), C8 (no overflow).
 */
struct gale_condvar_broadcast_decision gale_k_condvar_broadcast_decide(uint32_t num_waiters);

/* ---- Wait decision ---- */

struct gale_condvar_wait_decision {
    uint8_t action; /* 0 = PEND_CURRENT, 1 = RETURN_EAGAIN */
    int32_t ret;    /* return code for RETURN_EAGAIN path */
};

#define GALE_CONDVAR_WAIT_PEND         0
#define GALE_CONDVAR_WAIT_RETURN_EAGAIN 1

/**
 * Decide action for k_condvar_wait.
 *
 * @param is_no_wait  Non-zero if timeout is K_NO_WAIT.
 * @return            PEND_CURRENT or RETURN_EAGAIN with ret = -EAGAIN.
 *
 * Verified: C6 (thread queued on blocking path).
 */
struct gale_condvar_wait_decision gale_k_condvar_wait_decide(uint32_t is_no_wait);

#ifdef __cplusplus
}
#endif

#endif /* GALE_CONDVAR_H_ */
