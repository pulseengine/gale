/*
 * SPDX-License-Identifier: Apache-2.0
 * Copyright (c) 2026 PulseEngine
 *
 * STM32G4 MCU self-monitoring — die temperature and internal
 * reference voltage via ADC1.
 *
 * Calibration locations and formulas per STMicroelectronics
 * RM0440 (STM32G4 reference manual) §3.7.1 "Boot and start-up
 * memory areas" and §21 "ADC".
 *
 *   TS_CAL1     at 0x1FFF75A8 — TS reading at 30 °C, VDDA = 3.0 V
 *   TS_CAL2     at 0x1FFF75CA — TS reading at 130 °C, VDDA = 3.0 V
 *   VREFINT_CAL at 0x1FFF75AA — VREFINT reading at 30 °C, 3.0 V
 *
 * Formulas (RM0440 §21.4.32, §21.4.33):
 *
 *   actual_VREF_mV = 3000 * VREFINT_CAL / raw_VREFINT
 *
 *   temp_C = ((TS_CAL2_TEMP_C - TS_CAL1_TEMP_C) * (raw_TS_at_3V0 - TS_CAL1))
 *          / (TS_CAL2 - TS_CAL1) + TS_CAL1_TEMP_C
 *
 *   raw_TS_at_3V0 = raw_TS * actual_VREF_mV / 3000
 *     (correct the TS reading back to the calibration condition)
 *
 * VBAT is not exposed as an ADC channel on this Nucleo configuration
 * (no battery input), so we always report 0 for vbat_mV.
 */

#include <stdint.h>
#include <stdio.h>
#include <zephyr/devicetree.h>
#include <zephyr/drivers/adc.h>
#include <zephyr/sys/util.h>

#include "smart_mcu.h"

#define ADC_NODE          DT_NODELABEL(adc1)
#define ADC_RESOLUTION    12

#define ADC_CH_TS         16
#define ADC_CH_VREFINT    18

#define TS_CAL1_ADDR      ((const volatile uint16_t *)0x1FFF75A8u)
#define TS_CAL2_ADDR      ((const volatile uint16_t *)0x1FFF75CAu)
#define VREFINT_CAL_ADDR  ((const volatile uint16_t *)0x1FFF75AAu)
#define TS_CAL1_TEMP_C    30
#define TS_CAL2_TEMP_C    130
#define CAL_VREF_MV       3000

static const struct device *s_adc;
static int s_init_ok;

static int read_one(uint8_t channel_id, int16_t *out)
{
	int16_t buf;
	struct adc_sequence seq = {
		.channels    = BIT(channel_id),
		.buffer      = &buf,
		.buffer_size = sizeof(buf),
		.resolution  = ADC_RESOLUTION,
	};
	int ret = adc_read(s_adc, &seq);
	if (ret == 0) {
		*out = buf;
	}
	return ret;
}

static int setup_channel(uint8_t channel_id)
{
	struct adc_channel_cfg cfg = {
		.gain             = ADC_GAIN_1,
		.reference        = ADC_REF_INTERNAL,
		.acquisition_time = ADC_ACQ_TIME_MAX,
		.channel_id       = channel_id,
		.differential     = 0,
	};
	return adc_channel_setup(s_adc, &cfg);
}

int smart_mcu_init(void)
{
	s_adc = DEVICE_DT_GET(ADC_NODE);
	if (!device_is_ready(s_adc)) {
		printf("# H init: ADC1 device not ready — DT overlay missing?\n");
		return -1;
	}
	if (setup_channel(ADC_CH_TS) != 0) {
		printf("# H init: ADC1 ch%u (TS) setup failed\n", ADC_CH_TS);
		return -2;
	}
	if (setup_channel(ADC_CH_VREFINT) != 0) {
		printf("# H init: ADC1 ch%u (VREFINT) setup failed\n",
		       ADC_CH_VREFINT);
		return -3;
	}
	s_init_ok = 1;
	printf("# H ready: temp_mC,vref_mV via ADC1 ch%u + ch%u; "
	       "VBAT not wired (always 0)\n",
	       ADC_CH_TS, ADC_CH_VREFINT);
	printf("# H cal: TS_CAL1=%u@%dC TS_CAL2=%u@%dC VREFINT_CAL=%u@%dmV\n",
	       (unsigned)*TS_CAL1_ADDR, TS_CAL1_TEMP_C,
	       (unsigned)*TS_CAL2_ADDR, TS_CAL2_TEMP_C,
	       (unsigned)*VREFINT_CAL_ADDR, CAL_VREF_MV);
	return 0;
}

void smart_mcu_snapshot(struct mcu_health *out)
{
	out->vbat_mV = 0;

	if (!s_init_ok) {
		out->temp_mC = 0;
		out->vref_mV = 0;
		return;
	}

	int16_t raw_ts = 0, raw_vref = 0;
	if (read_one(ADC_CH_VREFINT, &raw_vref) != 0 || raw_vref == 0) {
		out->temp_mC = 0;
		out->vref_mV = 0;
		return;
	}
	if (read_one(ADC_CH_TS, &raw_ts) != 0) {
		out->temp_mC = 0;
		out->vref_mV = (uint32_t)CAL_VREF_MV
		             * (uint32_t)*VREFINT_CAL_ADDR
		             / (uint32_t)raw_vref;
		return;
	}

	uint32_t vref_cal = *VREFINT_CAL_ADDR;
	uint32_t actual_vref_mV = (uint32_t)CAL_VREF_MV * vref_cal
	                        / (uint32_t)raw_vref;

	/* Correct TS reading to the calibration's 3.0 V condition.
	 * raw_TS_at_3V0 = raw_TS * actual_VREF / 3.0V */
	int32_t raw_ts_at_3v0 = (int32_t)raw_ts
	                      * (int32_t)actual_vref_mV
	                      / CAL_VREF_MV;

	int32_t ts_cal1 = (int32_t)*TS_CAL1_ADDR;
	int32_t ts_cal2 = (int32_t)*TS_CAL2_ADDR;
	int32_t span_cal = ts_cal2 - ts_cal1;
	if (span_cal == 0) {
		/* Defensive: corrupted calibration ROM. */
		out->temp_mC = 0;
	} else {
		int32_t temp_C_x1000 =
			(int32_t)(TS_CAL2_TEMP_C - TS_CAL1_TEMP_C) * 1000
			* (raw_ts_at_3v0 - ts_cal1) / span_cal
			+ TS_CAL1_TEMP_C * 1000;
		out->temp_mC = temp_C_x1000;
	}
	out->vref_mV = actual_vref_mV;
}

void smart_mcu_emit(const char *at, const struct mcu_health *h)
{
	printf("H,%s,%d,%u,%u\n",
	       at, (int)h->temp_mC, (unsigned)h->vref_mV,
	       (unsigned)h->vbat_mV);
}
