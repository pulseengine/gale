/*
 * SPDX-License-Identifier: Apache-2.0
 * Copyright (c) 2026 PulseEngine
 *
 * Engine control algorithm: ignition advance + fuel duration.
 * Realistic-ish but deliberately bounded — only table lookups +
 * small integer arithmetic, no divides, no allocation. The point
 * is to exercise the Zephyr primitive chain around the ISR, not
 * to be an actual ECU.
 */

#ifndef GALE_BENCH_CONTROL_H
#define GALE_BENCH_CONTROL_H

#include <stdint.h>

/* Per-ISR sample — what the "crank" writes to the ring buffer.
 * Three timestamps let us separate "pure algorithm" from "primitive
 * handoff" costs — only the latter should differ between baseline
 * and Gale builds. */
struct crank_sample {
	uint32_t angle_deg;        /* 0..719 for a 4-stroke cycle */
	uint32_t rpm;              /* current engine speed */
	int16_t  spark_advance_deg;/* computed by control_step */
	uint16_t fuel_duration_us; /* computed by control_step */
	uint32_t t_entry;          /* cycle counter at ISR entry */
	uint32_t t_algo_end;       /* after control_step() */
	uint32_t t_exit;           /* after ring_buf_put + k_sem_give */
	uint16_t drops_so_far;     /* monotonic ring-buffer drop count */
	uint16_t seq;              /* sequence number (wraps) */
};

/* Input vector sampled by the ISR from simulated sensors. */
struct engine_state {
	uint32_t rpm;
	uint16_t load_pct;      /* 0..100, simulated MAP */
	int16_t  coolant_c;     /* -40..+120 */
	uint8_t  knock_retard;  /* 0..15 degrees */
};

/*
 * Pure function: given sensor state, compute ignition advance and
 * fuel injection duration. No side effects.
 *
 * - spark_advance_deg: degrees before top dead centre (BTDC)
 * - fuel_duration_us: injector pulse width
 */
void control_step(const struct engine_state *in,
		  int16_t *spark_advance_deg,
		  uint16_t *fuel_duration_us);

#endif /* GALE_BENCH_CONTROL_H */
