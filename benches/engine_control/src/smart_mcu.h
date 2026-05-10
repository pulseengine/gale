/*
 * SPDX-License-Identifier: Apache-2.0
 * Copyright (c) 2026 PulseEngine
 *
 * MCU self-monitoring snapshot — vendor-neutral interface.
 *
 * "Smart data" beyond DWT counters: did the chip stay in spec while
 * the bench was running? A capture made at +25 °C on cold silicon
 * and one made at +85 °C with the regulator drooping are not the
 * same measurement.  We record the chip's own telemetry so the
 * analyzer can flag a thermal- or supply-related anomaly rather
 * than mis-attributing it to the gale primitives.
 *
 * Each backend (smart_mcu_g4.c, smart_mcu_stub.c) implements this
 * interface for its target. The stub is selected for any board
 * that doesn't have a vendor-specific backend; it returns the
 * "not available" marker so the CSV row format stays the same.
 *
 *   H,<at>,<temp_mC>,<vref_mV>,<vbat_mV>
 *
 * `vbat_mV` is reported as 0 on parts without a VBAT divider channel.
 * Any field marked unavailable on a given backend is reported as 0
 * with a one-time `# H <field>: not available on this target`
 * comment line at boot.
 */

#ifndef GALE_BENCH_SMART_MCU_H
#define GALE_BENCH_SMART_MCU_H

#include <stdint.h>

struct mcu_health {
	int32_t  temp_mC;   /* die temperature in milli-degrees C */
	uint32_t vref_mV;   /* internal reference voltage, milli-volts */
	uint32_t vbat_mV;   /* VBAT pin voltage, mV; 0 if unavailable */
};

/* One-time init: opens the ADC + reads vendor calibration ROM.
 * Returns 0 on success; non-zero leaves the backend in
 * "snapshot returns zeroed health" mode (still safe to call).
 * Always emits a banner comment summarising what's available. */
int smart_mcu_init(void);

/* Snapshot the current readings. Reads ADC channels — costs a few
 * tens of microseconds. Safe from thread context only. */
void smart_mcu_snapshot(struct mcu_health *out);

/* Emit one CSV row:
 *   H,<at>,<temp_mC>,<vref_mV>,<vbat_mV> */
void smart_mcu_emit(const char *at, const struct mcu_health *h);

#endif /* GALE_BENCH_SMART_MCU_H */
