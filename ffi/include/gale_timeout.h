/*
 * Gale Timeout FFI — verified tick arithmetic and deadline tracking.
 *
 * Phase 2: Decision struct pattern for Extract->Decide->Apply.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#ifndef GALE_TIMEOUT_H
#define GALE_TIMEOUT_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ---- Phase 2: Timeout Decision API ---- */

/**
 * Decision struct for z_add_timeout — computed deadline.
 */
struct gale_timeout_add_decision {
    int32_t ret;        /* 0 (OK), -EINVAL (overflow) */
    uint64_t deadline;  /* absolute deadline (valid when ret == 0) */
};

/**
 * Compute absolute deadline from current tick + duration.
 *
 * Verified: TO2 (deadline = current_tick + duration), TO5 (no overflow).
 */
struct gale_timeout_add_decision gale_timeout_add_decide(
    uint64_t current_tick, uint64_t duration);

/**
 * Decision struct for z_abort_timeout — whether abort is valid.
 */
struct gale_timeout_abort_decision {
    int32_t ret;    /* 0 (OK, was active), -EINVAL (already inactive) */
    uint8_t action; /* 0=DO_REMOVE, 1=NOOP */
};

#define GALE_TIMEOUT_ACTION_REMOVE 0
#define GALE_TIMEOUT_ACTION_NOOP   1

/**
 * Decide whether to abort a pending timeout.
 *
 * @param is_linked  1 if timeout node is linked (active), 0 if inactive.
 *
 * Verified: TO3 (abort clears to inactive).
 */
struct gale_timeout_abort_decision gale_timeout_abort_decide(
    uint32_t is_linked);

/**
 * Decision struct for sys_clock_announce — new tick and expiry status.
 */
struct gale_timeout_announce_decision {
    int32_t ret;      /* 0 (OK), -EINVAL (overflow) */
    uint64_t new_tick; /* advanced tick value */
    uint32_t fired;    /* 1 if expired, 0 otherwise */
};

/**
 * Advance tick and check if a timeout has expired.
 *
 * Verified: TO4 (fires when deadline <= now), TO5 (no overflow),
 *           TO7 (K_FOREVER never expires).
 */
struct gale_timeout_announce_decision gale_timeout_announce_decide(
    uint64_t current_tick, uint64_t ticks,
    uint64_t deadline, uint32_t active);

/* ---- Legacy API (kept for backward compatibility) ---- */

/**
 * Schedule a timeout: compute absolute deadline from current tick + duration.
 *
 * @param current_tick  Current system tick.
 * @param duration      Relative timeout in ticks.
 * @param deadline      Output: absolute deadline.
 *
 * @return 0 on success, -EINVAL on overflow or null pointer.
 */
int32_t gale_timeout_add(uint64_t current_tick,
                          uint64_t duration,
                          uint64_t *deadline);

/**
 * Abort a pending timeout.
 *
 * @param active  1 if timeout is active, 0 if inactive.
 *
 * @return 0 on success (was active), -EINVAL if already inactive.
 */
int32_t gale_timeout_abort(uint32_t active);

/**
 * Advance tick and check if a timeout has expired.
 *
 * @param current_tick  Current system tick.
 * @param ticks         Ticks to advance.
 * @param deadline      Absolute deadline of this timeout.
 * @param active        1 if timeout is active.
 * @param new_tick      Output: advanced tick value.
 * @param fired         Output: 1 if expired, 0 otherwise.
 *
 * @return 0 on success, -EINVAL on overflow or null pointer.
 */
int32_t gale_timeout_announce(uint64_t current_tick,
                               uint64_t ticks,
                               uint64_t deadline,
                               uint32_t active,
                               uint64_t *new_tick,
                               uint32_t *fired);

#ifdef __cplusplus
}
#endif

#endif /* GALE_TIMEOUT_H */
