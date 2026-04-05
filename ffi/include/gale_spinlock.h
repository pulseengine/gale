/*
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale spinlock — verified nesting-discipline state machine.
 *
 * These functions model the high-level ownership and nesting depth
 * tracking from src/spinlock.rs.  The low-level encoding checks
 * (gale_spinlock_validate.h) remain separate.
 *
 * All spinlock primitives (hardware CAS/ticket, IRQ masking, memory
 * barriers) remain in Zephyr C.  Only the nesting-state invariants
 * are verified here.
 *
 * Verified properties (Verus proofs):
 *   SL1: lock acquired only when free
 *   SL2: release only by current owner
 *   SL3: nest_count tracks depth correctly
 *   SL4: fully released when nest_count reaches 0
 *   SL5: double-acquire without nesting is rejected
 */

#ifndef GALE_SPINLOCK_H_
#define GALE_SPINLOCK_H_

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Check whether acquiring the spinlock is valid (non-recursive).
 *
 * @param owner_tid  Current owner thread ID, 0 if free.
 * @return           1 if valid (lock is free), 0 if already held.
 *
 * SL1: only valid when owner_tid == 0.
 * SL5: double-acquire by any thread rejected.
 */
int32_t gale_spinlock_acquire_check(uint32_t owner_tid);

/**
 * Acquire the spinlock (non-recursive).
 *
 * If free (owner_tid == 0): writes new_tid to *out_owner,
 * writes 1 to *out_nest_count, returns 0.
 * If held: returns -EBUSY without modifying out values.
 *
 * @param owner_tid      Current owner thread ID (0 = free).
 * @param nest_count     Current nesting depth (unused for non-recursive).
 * @param new_tid        Thread ID of the acquiring thread.
 * @param out_nest_count Written with new nesting depth on success.
 * @param out_owner      Written with new_tid on success.
 * @return               0 on success, -EBUSY (-16) if already held.
 *
 * SL1: free -> acquired, nest_count = 1. SL3.
 */
int32_t gale_spinlock_acquire(uint32_t owner_tid, uint32_t nest_count,
                               uint32_t new_tid,
                               uint32_t *out_nest_count, uint32_t *out_owner);

/**
 * Acquire the spinlock with nesting support.
 *
 * Free: acquires with nest_count = 1.
 * Same owner, room to nest: increments nest_count.
 * Same owner at MAX_NEST_DEPTH: returns -EBUSY.
 * Different owner: returns -EBUSY.
 *
 * @param owner_tid      Current owner thread ID (0 = free).
 * @param nest_count     Current nesting depth.
 * @param new_tid        Thread ID of the acquiring thread.
 * @param out_nest_count Written with new nesting depth on success.
 * @param out_owner      Written with new owner on success.
 * @return               0 on success, -EBUSY (-16) otherwise.
 *
 * SL1, SL3: nesting depth tracked correctly.
 */
int32_t gale_spinlock_acquire_nested(uint32_t owner_tid, uint32_t nest_count,
                                      uint32_t new_tid,
                                      uint32_t *out_nest_count, uint32_t *out_owner);

/**
 * Release the spinlock.
 *
 * Only the current owner (tid == owner_tid) may release.
 * Final release (nest_count <= 1): clears owner and nest_count to 0.
 * Nested release (nest_count > 1): decrements nest_count.
 *
 * @param owner_tid      Current owner thread ID.
 * @param nest_count     Current nesting depth.
 * @param tid            Thread ID of the releasing thread.
 * @param out_nest_count Written with new nesting depth on success.
 * @param out_owner      Written with new owner (0 on final release).
 * @return               0 on success, -EPERM (-1) if not owner.
 *
 * SL2: only owner can release. SL3, SL4.
 */
int32_t gale_spinlock_release(uint32_t owner_tid, uint32_t nest_count,
                               uint32_t tid,
                               uint32_t *out_nest_count, uint32_t *out_owner);

/**
 * Check whether the spinlock is currently held.
 *
 * @param owner_tid  Current owner thread ID, 0 if free.
 * @return           1 if held, 0 if free.
 */
int32_t gale_spinlock_is_held(uint32_t owner_tid);

/**
 * Get the current nesting depth.
 *
 * @param nest_count  Current nesting depth field value.
 * @return            The nesting depth (same value passed in).
 */
uint32_t gale_spinlock_nest_depth(uint32_t nest_count);

#ifdef __cplusplus
}
#endif

#endif /* GALE_SPINLOCK_H_ */
