/*
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale atomic — verified arithmetic decision functions for kernel/atomic_c.c.
 *
 * These functions replace the value-transformation logic in atomic_c.c.
 * The spinlock-based atomicity (k_spin_lock/k_spin_unlock, IRQ masking)
 * remains in the C shim — Rust only decides the arithmetic result.
 *
 * Verified operations (Verus proofs):
 *   AT1: add returns old value, stores old + val (wrapping)
 *   AT2: sub returns old value, stores old - val (wrapping)
 *   AT3: cas succeeds only when current == expected
 *   AT4: cas failure leaves value unchanged
 *   AT6: wrapping semantics for add/sub
 */

#ifndef GALE_ATOMIC_H_
#define GALE_ATOMIC_H_

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ---- Read-modify-write decision ---- */

/**
 * Result of a read-modify-write atomic operation (add/sub/or/and/xor/nand/set).
 *
 * C extracts: current value of *target under spinlock.
 * Rust computes: old (return to caller) + new_val (to store back).
 * C applies: *target = new_val; return (atomic_val_t)old_val;
 */
struct gale_atomic_rmw_decision {
    uint32_t old_val; /* returned to the caller of the atomic operation */
    uint32_t new_val; /* value to write back to *target */
};

/* ---- Compare-and-swap decision ---- */

/**
 * Result of a compare-and-swap operation.
 *
 * C applies: if (success) *target = new_val; return (bool)success;
 */
struct gale_atomic_cas_decision {
    uint32_t success; /* 1 = swapped, 0 = not swapped */
    uint32_t new_val; /* valid only when success == 1 */
};

/* ---- Function declarations ---- */

/** Atomic get — pass through current value. No modification. */
uint32_t gale_atomic_get(uint32_t current);

/** Atomic set — write new value, return old. */
struct gale_atomic_rmw_decision gale_atomic_set(uint32_t current, uint32_t value);

/** Atomic add — wrapping add, return old. AT1, AT6. */
struct gale_atomic_rmw_decision gale_atomic_add(uint32_t current, uint32_t value);

/** Atomic sub — wrapping sub, return old. AT2, AT6. */
struct gale_atomic_rmw_decision gale_atomic_sub(uint32_t current, uint32_t value);

/** Atomic OR — bitwise OR, return old. */
struct gale_atomic_rmw_decision gale_atomic_or(uint32_t current, uint32_t value);

/** Atomic AND — bitwise AND, return old. */
struct gale_atomic_rmw_decision gale_atomic_and(uint32_t current, uint32_t value);

/** Atomic XOR — bitwise XOR, return old. */
struct gale_atomic_rmw_decision gale_atomic_xor(uint32_t current, uint32_t value);

/** Atomic NAND — ~(current & value), return old. */
struct gale_atomic_rmw_decision gale_atomic_nand(uint32_t current, uint32_t value);

/** Atomic CAS — succeed iff current == expected. AT3, AT4. */
struct gale_atomic_cas_decision gale_atomic_cas(
    uint32_t current, uint32_t expected, uint32_t new_value);

#ifdef __cplusplus
}
#endif

#endif /* GALE_ATOMIC_H_ */
