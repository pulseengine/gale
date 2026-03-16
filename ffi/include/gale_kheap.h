/*
 * Gale KHeap FFI — verified byte-level allocation tracking.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#ifndef GALE_KHEAP_H
#define GALE_KHEAP_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Validate a kheap allocation and compute new allocated_bytes.
 *
 * @param allocated_bytes  Current bytes allocated.
 * @param capacity         Total heap capacity in bytes.
 * @param bytes            Bytes requested.
 * @param new_allocated    Output: updated allocated count.
 *
 * @return 0 on success, -ENOMEM if exceeds capacity, -EINVAL on error.
 */
int32_t gale_kheap_alloc_validate(uint32_t allocated_bytes,
                                   uint32_t capacity,
                                   uint32_t bytes,
                                   uint32_t *new_allocated);

/**
 * Validate a kheap free and compute new allocated_bytes.
 *
 * @param allocated_bytes  Current bytes allocated.
 * @param bytes            Bytes to free.
 * @param new_allocated    Output: updated allocated count.
 *
 * @return 0 on success, -EINVAL on underflow or error.
 */
int32_t gale_kheap_free_validate(uint32_t allocated_bytes,
                                  uint32_t bytes,
                                  uint32_t *new_allocated);

#ifdef __cplusplus
}
#endif

#endif /* GALE_KHEAP_H */
