/*
 * Copyright (c) 2018 Intel Corporation
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale timeout — phase 2: Extract->Decide->Apply pattern.
 *
 * This is kernel/timeout.c with the safety-critical tick arithmetic
 * delegated to Rust decision structs.  C extracts kernel state
 * (spinlock, timeout list, hardware timer), Rust decides the
 * arithmetic result, C applies it.
 *
 * Verified operations (Verus proofs):
 *   gale_timeout_add_decide      — TO2 (deadline computation), TO5 (no overflow)
 *   gale_timeout_abort_decide    — TO3 (deactivate)
 *   gale_timeout_announce_decide — TO4 (fire expired), TO5 (no overflow),
 *                                  TO7 (K_FOREVER never expires)
 *
 * All other timeout logic (linked-list, spinlock, callbacks, hardware timer)
 * remains native Zephyr C — only the arithmetic is verified.
 */

#include "gale_timeout.h"

/*
 * The decision-struct FFI functions are linked directly from libgale_ffi.a:
 *
 *   gale_timeout_add_decide()      — compute deadline from tick + duration
 *   gale_timeout_abort_decide()    — validate abort (is node linked?)
 *   gale_timeout_announce_decide() — advance tick, check expiry
 *
 * This file serves as the Zephyr module build target.  The actual
 * integration point is in the modified kernel/timeout.c, which calls
 * these decision functions at the Extract->Decide->Apply boundaries.
 *
 * Example integration in kernel/timeout.c z_add_timeout():
 *
 *   // Extract: read curr_tick, compute duration
 *   uint64_t tick = curr_tick;
 *   uint64_t dur  = timeout.ticks + 1 + ticks_elapsed;
 *
 *   // Decide: Rust validates overflow and computes deadline
 *   struct gale_timeout_add_decision d =
 *       gale_timeout_add_decide(tick, dur);
 *   if (d.ret != 0) { return 0; }  // overflow — treat as forever
 *
 *   // Apply: set dticks from verified deadline
 *   to->dticks = d.deadline - curr_tick;
 *
 * Example integration in kernel/timeout.c z_abort_timeout():
 *
 *   // Extract: check if node is linked
 *   uint32_t linked = sys_dnode_is_linked(&to->node) ? 1U : 0U;
 *
 *   // Decide: Rust validates the abort
 *   struct gale_timeout_abort_decision d =
 *       gale_timeout_abort_decide(linked);
 *
 *   // Apply: only remove if Rust says DO_REMOVE
 *   if (d.action == GALE_TIMEOUT_ACTION_REMOVE) {
 *       remove_timeout(to);
 *       to->dticks = TIMEOUT_DTICKS_ABORTED;
 *       ret = 0;
 *   }
 *
 * Example integration in kernel/timeout.c sys_clock_announce():
 *
 *   // Extract: read curr_tick, dticks of first timeout
 *   uint64_t tick = curr_tick;
 *   uint64_t dt   = (uint64_t)t->dticks;
 *
 *   // Decide: Rust computes new tick and checks expiry
 *   struct gale_timeout_announce_decision d =
 *       gale_timeout_announce_decide(tick, dt, deadline, active);
 *
 *   // Apply: advance tick, fire if decided
 *   if (d.ret == 0) {
 *       curr_tick = d.new_tick;
 *       if (d.fired) { remove_timeout(t); t->fn(t); }
 *   }
 */
