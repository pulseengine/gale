/*
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale sched — verified scheduler primitives.
 *
 * This C shim provides the glue between Zephyr's scheduler
 * and the formally verified Rust FFI. Run queue data structures,
 * thread state transitions, wait queues, and SMP IPI remain in Zephyr.
 *
 * Verified operations (Verus proofs):
 *   gale_sched_next_up          — SC5 (highest-priority), SC7 (idle fallback)
 *   gale_sched_should_preempt   — SC6 (cooperative protection)
 */

#include "gale_sched.h"
