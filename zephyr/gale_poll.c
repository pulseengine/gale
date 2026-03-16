/*
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale poll — verified poll event state machine and signal.
 *
 * This C shim provides the glue between Zephyr's poll subsystem
 * and the formally verified Rust FFI. Wait queue management, thread
 * scheduling, and work queue integration remain in Zephyr.
 *
 * Verified operations (Verus proofs):
 *   gale_poll_event_init   — PL1 (NOT_READY init)
 *   gale_poll_check_sem    — PL3 (SEM_AVAILABLE iff count > 0)
 *   gale_poll_signal_raise — PL7 (set signaled + result)
 *   gale_poll_signal_reset — PL8 (clear signaled)
 */

#include "gale_poll.h"
