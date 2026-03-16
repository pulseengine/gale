/*
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale timeslice — verified tick accounting for preemptive scheduling.
 *
 * This C shim provides the glue between Zephyr's timeslicing subsystem
 * and the formally verified Rust FFI. Timeout scheduling, IPI dispatch,
 * and per-CPU arrays remain in Zephyr.
 *
 * Verified operations (Verus proofs):
 *   gale_timeslice_reset — TS2 (reset to max)
 *   gale_timeslice_tick  — TS3 (decrement), TS4 (expire), TS5 (no underflow)
 */

#include "gale_timeslice.h"
