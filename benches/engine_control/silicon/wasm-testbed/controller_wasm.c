/*
 * Value-in/value-out wasm shim of flight_control's controller_step().
 * Scalars in (the 6 flight_state fields it reads + updates), packed u32 out.
 * No host pointer, no memory, no tables — exercises signed arithmetic-shift
 * (SAR), saturation clamps, and int8 bit-packing: codegen the engine_control
 * algorithm and the sem primitive did not cover.
 */
#include <stdint.h>

__attribute__((export_name("controller_step_decide")))
uint32_t controller_step_decide(int32_t roll_mdeg, int32_t roll_rate,
				int32_t pitch_mdeg, int32_t pitch_rate,
				int32_t yaw_mdeg, int32_t yaw_rate,
				uint32_t updates)
{
	int32_t aileron  = -(roll_mdeg  >> 6) - (roll_rate  >> 7);
	int32_t elevator = -(pitch_mdeg >> 6) - (pitch_rate >> 7);
	int32_t rudder   = -(yaw_mdeg   >> 6) - (yaw_rate   >> 7);

	if (aileron  >  127) aileron  =  127;
	if (aileron  < -127) aileron  = -127;
	if (elevator >  127) elevator =  127;
	if (elevator < -127) elevator = -127;
	if (rudder   >  127) rudder   =  127;
	if (rudder   < -127) rudder   = -127;

	uint32_t cmd = ((uint32_t)(uint8_t)(int8_t)aileron        )
		     | ((uint32_t)(uint8_t)(int8_t)elevator <<  8)
		     | ((uint32_t)(uint8_t)(int8_t)rudder   << 16)
		     | ((uint32_t)(updates & 0xFFu)         << 24);
	return cmd;
}
