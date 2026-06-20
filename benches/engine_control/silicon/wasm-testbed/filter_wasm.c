/*
 * Value-in/value-out slice of flight_control's filter_step() hot math:
 * one axis of the complementary filter. new = (gyro_term*980 + accel*20)/1000
 * where gyro_term = prev + gyro. Pure signed integer: exercises SIGNED
 * multiply + SIGNED division by the constant 1000 (sdiv) — a path neither the
 * engine algorithm (udiv) nor the controller (shifts only) covered.
 */
#include <stdint.h>
#define ALPHA_GYRO_PERMILLE   980
#define ALPHA_ACCEL_PERMILLE  20
__attribute__((export_name("filter_axis_decide")))
int32_t filter_axis_decide(int32_t prev_mdeg, int32_t gyro, int32_t accel)
{
	int32_t gyro_term = prev_mdeg + gyro;
	return (gyro_term * ALPHA_GYRO_PERMILLE + accel * ALPHA_ACCEL_PERMILLE) / 1000;
}
