/*
 * Value-in/value-out wasm shim of engine_control's control_step().
 *
 * The native control_step takes `const struct engine_state *in` and writes
 * two out-params; for the wasm->loom->synth route we want a pure scalar
 * signature (no host pointer deref) so the only memory the synth output
 * touches is the embedded lookup tables (wasm data segment). Outputs are
 * packed into a u32: spark_advance (i16) in the high half, fuel (u16) low.
 *
 * Exercises synth codegen paths the `decide` primitive did NOT:
 *   - integer division by constants (rpm/500, load/5, *300/80, *enrich/1000)
 *   - reads of a static const data segment (the two lookup tables)
 */
#include <stdint.h>

#define RPM_BINS  20
#define LOAD_BINS 20

extern const int8_t  spark_advance_table[RPM_BINS][LOAD_BINS];
extern const uint16_t fuel_duration_table[RPM_BINS][LOAD_BINS];

static inline uint8_t rpm_bin(uint32_t rpm)
{
	uint32_t b = rpm / 500;
	if (b >= RPM_BINS) {
		b = RPM_BINS - 1;
	}
	return (uint8_t)b;
}

static inline uint8_t load_bin(uint16_t load_pct)
{
	uint16_t b = load_pct / 5;
	if (b >= LOAD_BINS) {
		b = LOAD_BINS - 1;
	}
	return (uint8_t)b;
}

static inline uint32_t coolant_enrichment_permille(int16_t coolant_c)
{
	if (coolant_c >= 80) {
		return 0;
	}
	if (coolant_c <= 0) {
		return 300;
	}
	return (uint32_t)((80 - coolant_c) * 300 / 80);
}

__attribute__((export_name("control_step_decide")))
uint32_t control_step_decide(uint32_t rpm, uint32_t load_pct,
			     int32_t coolant_c, uint32_t knock_retard)
{
	uint8_t rb = rpm_bin(rpm);
	uint8_t lb = load_bin((uint16_t)load_pct);

	int16_t advance = (int16_t)spark_advance_table[rb][lb];
	advance -= (int16_t)knock_retard;
	if (advance < 0) {
		advance = 0;
	}

	uint32_t base_fuel = (uint32_t)fuel_duration_table[rb][lb];
	uint32_t enrich = coolant_enrichment_permille((int16_t)coolant_c);
	uint32_t corrected = base_fuel + (base_fuel * enrich / 1000U);
	if (corrected > UINT16_MAX) {
		corrected = UINT16_MAX;
	}

	return ((uint32_t)(uint16_t)advance << 16) | (uint32_t)(uint16_t)corrected;
}
