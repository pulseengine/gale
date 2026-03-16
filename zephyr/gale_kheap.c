/*
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale kheap — verified byte-level allocation tracking.
 *
 * This C shim provides the glue between Zephyr's k_heap subsystem
 * and the formally verified Rust FFI. sys_heap internals, free-list
 * management, coalescing, wait queues, and tracing remain in Zephyr.
 *
 * Verified operations (Verus proofs):
 *   gale_kheap_alloc_validate — KH2 (alloc), KH3 (full), KH6 (no overflow)
 *   gale_kheap_free_validate  — KH4 (free), KH5 (conservation)
 */

#include "gale_kheap.h"
