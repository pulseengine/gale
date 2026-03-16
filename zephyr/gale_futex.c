/*
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale futex — verified fast userspace mutex value comparison.
 *
 * This C shim provides the glue between Zephyr's futex subsystem
 * and the formally verified Rust FFI. Spinlock, wait queue, thread
 * scheduling, and timeout handling remain in Zephyr.
 *
 * Verified operations (Verus proofs):
 *   gale_futex_wait_check — FX1/FX2 (value comparison gating)
 *   gale_futex_wake       — FX3/FX4/FX5 (wake count)
 */

#include "gale_futex.h"
