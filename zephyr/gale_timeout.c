/*
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale timeout — verified tick arithmetic and deadline tracking.
 *
 * This C shim provides the glue between Zephyr's timeout subsystem
 * and the formally verified Rust FFI. The actual timeout linked-list,
 * spinlock, callback dispatch, and hardware timer remain in Zephyr.
 *
 * Verified operations (Verus proofs):
 *   gale_timeout_add      — TO2 (deadline computation), TO5 (no overflow)
 *   gale_timeout_abort    — TO3 (deactivate)
 *   gale_timeout_announce — TO4 (fire expired), TO5 (no overflow)
 */

#include "gale_timeout.h"

/*
 * The FFI functions are linked directly from libgale_ffi.a.
 * This file exists as a build target for the Zephyr module system.
 * Kernel integration will call gale_timeout_add/abort/announce
 * from the modified timeout.c.
 */
