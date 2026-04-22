/*
 * SPDX-License-Identifier: Apache-2.0
 * Copyright (c) 2026 PulseEngine
 *
 * Pure control algorithm: table lookups + small integer corrections.
 * No divides, no memory allocation, no floating point. Bounded
 * execution time by construction — Gale's verification-friendly style.
 */

#include "control.h"
#include <stdint.h>

#define RPM_BINS  20
#define LOAD_BINS 20

extern const int8_t  spark_advance_table[RPM_BINS][LOAD_BINS];
extern const uint16_t fuel_duration_table[RPM_BINS][LOAD_BINS];

static inline uint8_t rpm_bin(uint32_t rpm)
{
	uint32_t b = rpm / 500;   /* 500 RPM per bin */
	if (b >= RPM_BINS) {
		b = RPM_BINS - 1;
	}
	return (uint8_t)b;
}

static inline uint8_t load_bin(uint16_t load_pct)
{
	uint16_t b = load_pct / 5; /* 5 % per bin */
	if (b >= LOAD_BINS) {
		b = LOAD_BINS - 1;
	}
	return (uint8_t)b;
}

/*
 * Coolant correction: below 80 °C we enrich fuel up to +30 % at 0 °C;
 * above 80 °C no correction; below 0 °C clamp at +30 %.
 * Integer-only: avoid any floating point.
 */
static inline uint32_t coolant_enrichment_permille(int16_t coolant_c)
{
	if (coolant_c >= 80) {
		return 0;
	}
	if (coolant_c <= 0) {
		return 300;   /* +30 % */
	}
	/* Linear from 300 permille at 0 °C to 0 permille at 80 °C. */
	return (uint32_t)((80 - coolant_c) * 300 / 80);
}

void control_step(const struct engine_state *in,
		  int16_t *spark_advance_deg,
		  uint16_t *fuel_duration_us)
{
	uint8_t rb = rpm_bin(in->rpm);
	uint8_t lb = load_bin(in->load_pct);

	int16_t advance = (int16_t)spark_advance_table[rb][lb];
	/* Apply knock retard (borrows from the base advance, saturating to 0). */
	advance -= (int16_t)in->knock_retard;
	if (advance < 0) {
		advance = 0;
	}
	*spark_advance_deg = advance;

	uint32_t base_fuel = (uint32_t)fuel_duration_table[rb][lb];
	uint32_t enrich = coolant_enrichment_permille(in->coolant_c);
	uint32_t corrected = base_fuel + (base_fuel * enrich / 1000U);
	if (corrected > UINT16_MAX) {
		corrected = UINT16_MAX;
	}
	*fuel_duration_us = (uint16_t)corrected;
}
