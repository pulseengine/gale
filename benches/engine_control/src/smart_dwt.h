/*
 * SPDX-License-Identifier: Apache-2.0
 * Copyright (c) 2026 PulseEngine
 *
 * DWT counter snapshot for the silicon-anchor protocol.
 *
 * On ARMv7-M the DWT (Data Watchpoint and Trace) unit exposes six
 * counters that quantify *why* a measured cycle count is what it
 * is — not just "how long did this take" but how much of it was
 * stalled on memory, on exceptions, on sleep, etc. A captured
 * silicon run that records these alongside the event timestamps
 * lets the analyzer discriminate "the runtime cost is real" from
 * "the runtime cost is a microarchitectural artefact".
 *
 *   CYCCNT   — full 32-bit cycle counter (wraps at 2^32)
 *   CPICNT   — 8-bit CPI overhead counter (extra cycles beyond 1 CPI)
 *   EXCCNT   — 8-bit exception entry/exit overhead counter
 *   SLEEPCNT — 8-bit sleep cycles counter
 *   LSUCNT   — 8-bit load/store unit additional cycles counter
 *   FOLDCNT  — 8-bit folded-instruction counter
 *
 * The five 8-bit counters wrap at 256, so the protocol takes a
 * snapshot at every RPM-step boundary; the analyzer computes
 * wrap-aware deltas. CYCCNT is wide enough to be safe across the
 * full sweep on any practical clock.
 *
 * On targets without a DWT unit (or where DWT is unimplemented in
 * the simulator — qemu_cortex_m3 in particular), all reads return
 * zero, so the row in the CSV is the literal value 0 across the
 * board and analyze.py treats that as "DWT not available" rather
 * than as a real measurement.
 */

#ifndef GALE_BENCH_SMART_DWT_H
#define GALE_BENCH_SMART_DWT_H

#include <stdint.h>

struct dwt_snapshot {
	uint32_t cyccnt;
	uint8_t  cpicnt;
	uint8_t  exccnt;
	uint8_t  sleepcnt;
	uint8_t  lsucnt;
	uint8_t  foldcnt;
};

/* Enable trace + all six counters. Idempotent. Must be called once
 * at boot before any snapshot. On targets without DWT this is a
 * no-op (the writes to memory-mapped registers are absorbed by the
 * simulator). */
void smart_dwt_enable(void);

/* Snapshot the current values. Cheap (six 32-bit MMIO reads). Safe
 * to call from thread or ISR context. */
void smart_dwt_snapshot(struct dwt_snapshot *out);

/* Emit one CSV row of the form
 *   D,<at>,<cyccnt>,<cpicnt>,<exccnt>,<sleepcnt>,<lsucnt>,<foldcnt>
 * where <at> is the event tag passed in (e.g. "boot", "step_3", "end"). */
void smart_dwt_emit(const char *at, const struct dwt_snapshot *s);

#endif /* GALE_BENCH_SMART_DWT_H */
