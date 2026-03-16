/*
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale thread lifecycle — verified create/exit counting and priority
 * validation.
 *
 * This C shim provides the glue between Zephyr's thread subsystem
 * and the formally verified Rust FFI. Stack setup, TLS, naming,
 * scheduling, and arch-specific initialization remain in Zephyr.
 *
 * Verified operations (Verus proofs):
 *   gale_thread_create_validate   — TH6 (no overflow)
 *   gale_thread_exit_validate     — TH5 (no underflow)
 *   gale_thread_priority_validate — TH1 (range check)
 */

#include "gale_thread_lifecycle.h"
