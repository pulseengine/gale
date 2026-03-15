/*
 * Gale Event FFI — verified bitmask operations.
 *
 * These functions replace the bitmask arithmetic in kernel/events.c.
 * Wait queues, scheduling, tracing, and userspace remain native Zephyr C.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#ifndef GALE_EVENT_H
#define GALE_EVENT_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Post (OR) new event bits into the bitmask.
 *
 * Computes: *result = events | new_events
 *
 * @param events     Current event bitmask.
 * @param new_events Bits to OR in.
 * @param result     Output: events | new_events.
 *
 * @return 0 on success, -EINVAL if result is NULL.
 */
int32_t gale_event_post(uint32_t events,
                        uint32_t new_events,
                        uint32_t *result);

/**
 * Set the event bitmask, returning the old value.
 *
 * Stores: *old_events = current (the caller applies new_events directly).
 *
 * @param new_events The new bitmask value (passed for API symmetry).
 * @param old_events Output: previous bitmask value.
 * @param current    Current event bitmask.
 *
 * @return 0 on success, -EINVAL if old_events is NULL.
 */
int32_t gale_event_set(uint32_t new_events,
                       uint32_t *old_events,
                       uint32_t current);

/**
 * Clear specific event bits.
 *
 * Computes: *result = events & ~clear_bits
 *
 * @param events     Current event bitmask.
 * @param clear_bits Bits to clear.
 * @param result     Output: events & ~clear_bits.
 *
 * @return 0 on success, -EINVAL if result is NULL.
 */
int32_t gale_event_clear(uint32_t events,
                         uint32_t clear_bits,
                         uint32_t *result);

/**
 * Set only the bits selected by a mask, leaving other bits unchanged.
 *
 * Computes: *result = (events & ~mask) | (new_bits & mask)
 *
 * @param events   Current event bitmask.
 * @param new_bits New values for the masked bits.
 * @param mask     Which bits to update.
 * @param result   Output: (events & ~mask) | (new_bits & mask).
 *
 * @return 0 on success, -EINVAL if result is NULL.
 */
int32_t gale_event_set_masked(uint32_t events,
                              uint32_t new_bits,
                              uint32_t mask,
                              uint32_t *result);

/**
 * Check if any of the desired event bits are set.
 *
 * @param events  Current event bitmask.
 * @param desired Bits to check.
 *
 * @return 1 if (events & desired) != 0, else 0.
 */
int32_t gale_event_wait_check_any(uint32_t events,
                                  uint32_t desired);

/**
 * Check if all of the desired event bits are set.
 *
 * @param events  Current event bitmask.
 * @param desired Bits to check.
 *
 * @return 1 if (events & desired) == desired, else 0.
 */
int32_t gale_event_wait_check_all(uint32_t events,
                                  uint32_t desired);

#ifdef __cplusplus
}
#endif

#endif /* GALE_EVENT_H */
