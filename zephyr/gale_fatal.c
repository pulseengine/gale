/*
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale fatal — verified fatal error classification.
 *
 * This C shim provides the glue between Zephyr's fatal error subsystem
 * and the formally verified Rust FFI. IRQ lock, coredump, thread abort,
 * and arch_system_halt remain in Zephyr.
 *
 * Verified operations (Verus proofs):
 *   gale_fatal_classify — FT1 (mapping), FT2 (panic halts), FT3 (recovery)
 */

#include "gale_fatal.h"
