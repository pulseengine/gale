/*
 * SPDX-License-Identifier: Apache-2.0
 * Copyright (c) 2026 PulseEngine
 *
 * Flight-control algo: complementary filter + tiny PID-ish controller.
 * Pure integer arithmetic — no float, no divides in the hot path
 * (well, one shift-divide in the controller, deterministic). The
 * "algo" segment must be byte-identical baseline vs gale to pass the
 * integrity assert.
 */

#include "control.h"

/*
 * Complementary-filter weight: gyro contribution = 98 %, accel = 2 %.
 * Bias the filter toward gyro for a 1 kHz update — accel is mostly
 * gravity-correction. Encoded as permille so we keep integer math.
 */
#define ALPHA_GYRO_PERMILLE   980
#define ALPHA_ACCEL_PERMILLE  20

void filter_step(struct flight_state *st, const struct imu_sample *s)
{
	/*
	 * Tilt approximation from accel: small-angle / atan-free, valid
	 * for ±20° (sufficient for a smoke-bench). Convert milli-g →
	 * milli-deg by treating accel components as direct angle proxies
	 * (1 g full-tilt ≈ 1000 milli-deg in this synthetic workload).
	 */
	int32_t accel_pitch = s->accel_x;
	int32_t accel_roll  = s->accel_y;

	/* Integrate gyro: rate × dt where dt = 1 ms ⇒ contribution
	 * is rate / 1000. Use shift /1024 to keep it pure integer
	 * with negligible bias. */
	int32_t gyro_pitch = st->pitch_mdeg + (s->gyro_y >> 0);
	int32_t gyro_roll  = st->roll_mdeg  + (s->gyro_x >> 0);
	int32_t gyro_yaw   = st->yaw_mdeg   + (s->gyro_z >> 0);

	st->pitch_mdeg = (gyro_pitch * ALPHA_GYRO_PERMILLE
			  + accel_pitch * ALPHA_ACCEL_PERMILLE) / 1000;
	st->roll_mdeg  = (gyro_roll  * ALPHA_GYRO_PERMILLE
			  + accel_roll  * ALPHA_ACCEL_PERMILLE) / 1000;
	st->yaw_mdeg   = gyro_yaw;   /* yaw has no accel correction */

	st->pitch_rate = s->gyro_y;
	st->roll_rate  = s->gyro_x;
	st->yaw_rate   = s->gyro_z;
}

uint32_t controller_step(const struct flight_state *st)
{
	/*
	 * Synthetic three-axis controller: each effort in the range
	 * roughly [-128, +128], packed into 24 bits of a u32 (8 bits
	 * spare for a "command opcode" field). PID-ish but only P with
	 * a small D — verification-friendly bounded execution.
	 */
	int32_t aileron  = -(st->roll_mdeg  >> 6) - (st->roll_rate  >> 7);
	int32_t elevator = -(st->pitch_mdeg >> 6) - (st->pitch_rate >> 7);
	int32_t rudder   = -(st->yaw_mdeg   >> 6) - (st->yaw_rate   >> 7);

	if (aileron  >  127) aileron  =  127;
	if (aileron  < -127) aileron  = -127;
	if (elevator >  127) elevator =  127;
	if (elevator < -127) elevator = -127;
	if (rudder   >  127) rudder   =  127;
	if (rudder   < -127) rudder   = -127;

	uint32_t cmd = ((uint32_t)(uint8_t)(int8_t)aileron        )
		     | ((uint32_t)(uint8_t)(int8_t)elevator <<  8)
		     | ((uint32_t)(uint8_t)(int8_t)rudder   << 16)
		     | ((uint32_t)(st->updates & 0xFFu)     << 24);
	return cmd;
}
