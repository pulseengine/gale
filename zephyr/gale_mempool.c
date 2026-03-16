/*
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale mempool — verified fixed-block pool allocation tracking.
 *
 * This C shim provides the glue between Zephyr's memory pool subsystem
 * and the formally verified Rust FFI. Bitarray management, alignment,
 * and actual memory allocation remain in Zephyr.
 *
 * Verified operations (Verus proofs):
 *   gale_mempool_alloc_validate — MP2 (alloc), MP3 (full)
 *   gale_mempool_free_validate  — MP4 (free), MP5 (conservation)
 */

#include "gale_mempool.h"
