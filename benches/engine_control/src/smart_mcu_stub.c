/*
 * SPDX-License-Identifier: Apache-2.0
 * Copyright (c) 2026 PulseEngine
 *
 * MCU self-monitoring — fallback stub for boards without a
 * vendor-specific backend (qemu_cortex_m3, mps2/an385, etc.).
 *
 * Every reading is reported as 0 / "unavailable". The CSV row
 * format stays identical so analyze.py can ingest captures from
 * stub and full-backend boards uniformly; the analyzer treats an
 * all-zero H row as "no MCU health on this target" rather than
 * "the chip is dead".
 */

#include <stdint.h>
#include <stdio.h>

#include "smart_mcu.h"

int smart_mcu_init(void)
{
	printf("# H temp_mC,vref_mV,vbat_mV: not available on this target "
	       "(stub backend)\n");
	return -1;
}

void smart_mcu_snapshot(struct mcu_health *out)
{
	out->temp_mC = 0;
	out->vref_mV = 0;
	out->vbat_mV = 0;
}

void smart_mcu_emit(const char *at, const struct mcu_health *h)
{
	printf("H,%s,%d,%u,%u\n",
	       at, (int)h->temp_mC, (unsigned)h->vref_mV, (unsigned)h->vbat_mV);
}
