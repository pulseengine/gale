/* Composed flight-control algo: the through-memory caller->callee seam for loom.
 * flight_algo() runs filter_step (writes *st) then controller_step (reads *st):
 * inlining controller_step here is exactly the memory-reading-callee case loom
 * described in loom#155. Exported so loom's inline pass has a real 2-fn seam. */
#include "control.h"
__attribute__((export_name("flight_algo")))
uint32_t flight_algo(struct flight_state *st, const struct imu_sample *s)
{
	filter_step(st, s);
	return controller_step(st);
}

/* Test driver: puts the structs in wasm linear memory (statics) and calls
 * flight_algo with their addresses, so a scalar-only runtime (wasmtime --invoke)
 * can exercise the through-memory path. Not part of the seam under test. */
static struct flight_state g_st;
static struct imu_sample   g_s;
__attribute__((export_name("drive")))
uint32_t drive(int32_t pitch,int32_t roll,int32_t yaw,
               int32_t gx,int32_t gy,int32_t gz,int32_t ax,int32_t ay)
{
	g_st = (struct flight_state){0};
	g_st.pitch_mdeg=pitch; g_st.roll_mdeg=roll; g_st.yaw_mdeg=yaw; g_st.updates=7;
	g_s = (struct imu_sample){0};
	g_s.gyro_x=gx; g_s.gyro_y=gy; g_s.gyro_z=gz; g_s.accel_x=ax; g_s.accel_y=ay;
	return flight_algo(&g_st, &g_s);
}
