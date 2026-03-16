/*
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale SMP state — verified SMP CPU state tracking.
 *
 * This C shim provides the glue between Zephyr's SMP subsystem
 * and the formally verified Rust FFI. IPI signaling, interrupt
 * stack setup, and arch CPU start remain in Zephyr.
 *
 * Verified operations (Verus proofs):
 *   gale_smp_start_cpu_validate — SM2 (start, active += 1)
 *   gale_smp_stop_cpu_validate  — SM3 (stop, CPU 0 never stops)
 */

#include "gale_smp_state.h"
