/*
 * SPDX-License-Identifier: Apache-2.0
 * Copyright (c) 2026 PulseEngine
 *
 * Scalar-ABI wrapper around the engine-control algorithm so the SAME
 * `control_step` (../src/control.c) can run unmodified in all three gale
 * wasm runtimes — browser / wasmtime+kiln / dissolved-native (wasm->loom->
 * synth->cortex-m). The wrapper packs the two outputs into one i32 so the
 * export needs no linear-memory pointer marshalling: a pure-scalar function
 * is trivially callable from `wasmtime run --invoke`, from JS, and from the
 * dissolved .o, which makes the 3-runtime differential a one-liner.
 *
 * This is the "real algorithm" optimization surface for synth/loom (gale#74,
 * task #26): table lookups (addressing modes), constant divides /500 /5 /80
 * /1000 (strength reduction), and saturating clamps (branch-on-flags) — every
 * pass the synth#390 codegen proposal targets, in a loop-free but
 * arithmetic-dense body. Measured native 180 B vs dissolved 378 B (2.1x),
 * a tighter gap than the gust scheduler hot path (3.9x) precisely because it
 * is arithmetic- rather than spill-dominated.
 */
#include "control.h"
#include <stdint.h>

/* spark(high 16) | fuel(low 16). */
unsigned control_step_packed(unsigned rpm, unsigned load_pct,
			     int coolant_c, unsigned knock)
{
	struct engine_state in;
	in.rpm = rpm;
	in.load_pct = (uint16_t)load_pct;
	in.coolant_c = (int16_t)coolant_c;
	in.knock_retard = (uint8_t)knock;

	int16_t spark;
	uint16_t fuel;
	control_step(&in, &spark, &fuel);
	return ((unsigned)(uint16_t)spark << 16) | (unsigned)fuel;
}
