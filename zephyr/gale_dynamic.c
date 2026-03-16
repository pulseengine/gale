/*
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale dynamic — verified dynamic thread pool tracking.
 *
 * This C shim provides the glue between Zephyr's dynamic thread pool
 * and the formally verified Rust FFI. Bitarray management, stack
 * allocation, and thread creation remain in Zephyr.
 *
 * Verified operations (Verus proofs):
 *   gale_dynamic_alloc_validate — DY2 (alloc), DY3 (full)
 *   gale_dynamic_free_validate  — DY4 (free, no underflow)
 */

#include "gale_dynamic.h"
