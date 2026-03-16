/*
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale work — verified work item state machine.
 *
 * This C shim provides the glue between Zephyr's work queue subsystem
 * and the formally verified Rust FFI. Queue management, scheduling,
 * handler dispatch, and delayable work remain in Zephyr.
 *
 * Verified operations (Verus proofs):
 *   gale_work_submit_validate — WK2 (queue), WK3 (reject cancel), WK4 (idempotent)
 *   gale_work_cancel_validate — WK5 (clear queued, set canceling)
 */

#include "gale_work.h"
