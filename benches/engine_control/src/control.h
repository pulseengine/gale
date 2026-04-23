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
 *
 * Post-#25 (event-stream methodology): carries the pre-handoff cycle
 * delta (algo_cycles) and the sweep-step index. The handoff cycle
 * count is measured AFTER ring_buf_put + k_sem_give, at which point
 * the sample is already in the ring — so it is published via a
 * side-channel array (main.c: g_handoff_by_slot) keyed by seq. The
 * reader pairs them up when emitting the event line. */
struct crank_sample {
	uint32_t angle_deg;        /* 0..719 for a 4-stroke cycle */
	uint32_t rpm;              /* current engine speed */
	uint32_t algo_cycles;      /* t_algo_end - t_entry, measured in ISR */
	int16_t  spark_advance_deg;/* computed by control_step */
	uint16_t fuel_duration_us; /* computed by control_step */
	uint16_t drops_so_far;     /* monotonic ring-buffer drop count */
	uint16_t seq;              /* sequence number (wraps); also indexes
	                            * the handoff side-channel slot */
	uint8_t  step;             /* sweep-step index (0..SWEEP_STEPS-1) */
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
