/*
 * SPDX-License-Identifier: Apache-2.0
 * Copyright (c) 2026 PulseEngine
 *
 * Flight-control macro benchmark — main app.
 *
 * Composes five Zephyr primitives (ring_buf, sem, mutex, msgq,
 * condvar) on a 100 Hz fixed-rate flight-control loop. See
 * docs/research/macro-bench-design.md for the design contract.
 *
 * Topology (Section 1 of the design doc):
 *
 *   sensor ISR (k_timer @ 1 kHz)
 *     -> ring_buf_put + k_sem_give    [algo + handoff measured]
 *        -> fusion thread (prio 4)
 *             -> k_mutex_lock(state)  [t_lock measured by controller]
 *                -> filter_step
 *             every 10 cycles: k_condvar_broadcast  [t_bcast measured]
 *
 *   controller ISR (k_timer @ 100 Hz)
 *     -> wakes controller thread (prio 5)
 *        -> k_mutex_lock(state)       [t_lock]
 *           -> snapshot state
 *        -> k_msgq_put(actuator_q)    [t_post]
 *           -> 3× actuator threads (prio 6/7/8)
 *              -> k_msgq_get -> work -> k_sem_give(done)
 *                 -> shared atomic timestamp [t_round measured at controller]
 *
 *   telemetry thread (prio 9, lowest)
 *     -> k_condvar_wait
 *        -> k_mutex_lock(state)       [priority-inheritance path]
 *
 * Per-controller-cycle event (rows that lack a controller pair-tag
 * are filtered out at emit; see emit_event for the rationale):
 *   E,<seq>,<step>,<load>,<algo>,<handoff>,<t_lock>,<t_post>,<t_round>,<t_bcast>
 *
 * `t_lock` is always populated on emitted rows (the filter key);
 * `t_post` / `t_round` / `t_bcast` are -1 if the matching primitive
 * didn't fire on this controller tick (e.g. broadcast every 10th
 * cycle => t_bcast = -1 on 9 of 10). `algo` and `handoff` are the
 * sensor-ISR-side numbers from the most recent ISR that wrote this
 * slot — strict superset of engine_control's schema; analyze.py
 * extends additively.
 *
 * Phase status (per the implementation plan):
 *   Phase 1 — sensor ISR + fusion thread + ring_buf + sem
 *   Phase 2 — controller + mutex + msgq + 3 actuators (t_lock, t_post)
 *   Phase 3 — condvar + telemetry (t_bcast)
 *   Phase 4 — 3-axis sweep driver
 *   Phase 5 — analyzer extension + CI wiring
 *
 * This file holds Phase 1–4. Phase 5 is in analyze.py + .github/.
 */

#include <zephyr/kernel.h>
#include <zephyr/sys/ring_buffer.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "control.h"

/* ------------------------------------------------------------- knobs */

#define RING_CAPACITY_SAMPLES   256

/* Side-channel slot count for the four cross-thread cycle deltas.
 * Sized at 512 to give >5× headroom over worst-case in-flight
 * samples at 2 kHz sensor × 100 Hz controller (per design doc
 * Section "Risks"). Using a power of two so `% RING_CAPACITY` is
 * a cheap mask. */
#define SLOT_COUNT              512

#define ACTUATOR_QUEUE_DEPTH    16
#define ACTUATOR_MSG_BYTES      64   /* msgq slot size; payload ≤ 64 B */

#define CONDVAR_BROADCAST_EVERY 10   /* every Nth fusion update */

/*
 * One step of the sweep. The driver iterates these and per-step
 * configures the sensor period, optional contention threads, and
 * msgq payload. Self-stops at `samples` per the audit-fixed pattern
 * engine_control already validates.
 */
struct sweep_step {
	uint16_t sensor_hz;     /* 500 / 1000 / 2000 */
	uint8_t  contention;    /* 0 / 1 / 2 extra noise threads */
	uint8_t  payload;       /* 16 / 32 / 64 bytes */
	uint16_t samples;       /* per-step sample budget */
};

/*
 * Default short sweep — QEMU smoke. The "long" sweep is the full
 * 3-axis 27-cell cartesian design from Section 4 of the design doc;
 * gated on ENGINE_BENCH_SWEEP=long via the CMakeLists.
 */
#if defined(ENGINE_BENCH_SWEEP_long)
/* Trimmed long sweep: 9 cells (sensor_hz=1000 only × 3 contention × 3 payload).
 *
 * Original 27-cell sweep blew the 120-min CI budget at step 13 in run
 * 25135494876 (sensor-rate UART emission overwhelmed Renode wall-time
 * at sensor_hz=2000). Two changes are applied together:
 *
 *   1. emit_event() now skips rows without a controller-cycle pair-tag
 *      (t_lock==0). At sensor_hz=1000 / ctrl=100 Hz, ~10% of sensor
 *      ISRs are emitted — UART traffic drops ~10×.
 *   2. Sweep restricted to sensor_hz=1000 (drop both 500 and 2000) so
 *      every cell has identical wall-time per ctrl-cycle. Single rate
 *      lands at engine_control's familiar regime; the contention ×
 *      payload axes carry the macro-bench signal.
 *
 * `samples` here is the sensor-ISR budget per cell. With the controller-
 * rate skip, expected ctrl-tagged rows per cell ≈ samples * 100 / 1000
 * = samples / 10. At samples=1000 we target ~100 ctrl-rows / cell.
 */
static const struct sweep_step sweep[] = {
	{ 1000, 0, 16, 1000 }, { 1000, 0, 32, 1000 }, { 1000, 0, 64, 1000 },
	{ 1000, 1, 16, 1000 }, { 1000, 1, 32, 1000 }, { 1000, 1, 64, 1000 },
	{ 1000, 2, 16, 1000 }, { 1000, 2, 32, 1000 }, { 1000, 2, 64, 1000 },
};
#else
/* QEMU smoke: 5 cells × 30 samples = 150 events. Same density as
 * engine_control's short sweep — ~30 s in QEMU. */
static const struct sweep_step sweep[] = {
	{  500, 0, 16, 30 },
	{ 1000, 0, 32, 30 },
	{ 1000, 1, 32, 30 },
	{ 2000, 0, 32, 30 },
	{ 2000, 2, 64, 30 },
};
#endif
#define SWEEP_STEPS (sizeof(sweep) / sizeof(sweep[0]))

#if defined(ENGINE_BENCH_SWEEP_long)
#  ifndef TOTAL_SAMPLES
/* 9 cells × ~100 ctrl-tagged rows each (skip-emission keeps only rows
 * with a controller-cycle pair-tag — see emit_event). Slack: budget
 * matches the cell-sample sum so the reader_loop terminates exactly
 * at the per-cell expected count, not before. */
#    define TOTAL_SAMPLES  900u
#  endif
#else
#  ifndef TOTAL_SAMPLES
/* 5 cells × ~10 ctrl-tagged rows ≈ 50; round to 50 for QEMU smoke. */
#    define TOTAL_SAMPLES  50u
#  endif
#endif

static inline uint32_t hz_to_period_us(uint32_t hz)
{
	if (hz == 0) {
		return 1000000U;
	}
	return 1000000U / hz;
}

/* ------------------------------------------------------------- state */

/*
 * IMU sensor input — the ISR reads g_imu_in (volatile, scalar 16-bit
 * fields, naturally atomic on this platform), runs the filter, and
 * pushes the result. Driven from the sweep driver to vary "load".
 */
static volatile int16_t g_imu_gyro_x = 0;
static volatile int16_t g_imu_gyro_y = 0;
static volatile int16_t g_imu_gyro_z = 0;
static volatile int16_t g_imu_accel_x = 0;
static volatile int16_t g_imu_accel_y = 0;
static volatile int16_t g_imu_accel_z = 0;

/* Per-step sweep parameters, written by the driver, read by ISRs +
 * threads. Same volatile-scalar pattern engine_control uses. */
static volatile uint8_t  g_step       = 0U;
static volatile uint16_t g_sensor_hz  = 1000U;
static volatile uint16_t g_load       = 0U;   /* aggregate load metric:
                                               * sensor_hz << 4 | (contention<<2) | payload_bin */
static volatile uint16_t g_payload    = 32U;
static volatile uint8_t  g_contention = 0U;

static volatile uint16_t g_seq        = 0U;
static volatile uint32_t g_sensor_ints = 0U;
static volatile uint16_t g_drops      = 0U;
static volatile uint32_t g_fire_budget = 0U;

/*
 * Side-channel slot arrays. The ISR/threads write into these by
 * `seq % SLOT_COUNT`; the reader copies them when emitting events.
 * Same SPSC ordering invariant engine_control already validates:
 * the slot is written BEFORE the matching ring sample is published
 * (k_sem_give wakes the consumer, but the ISR keeps running and
 * cannot be preempted by a thread on this single-CPU build).
 */
static volatile uint32_t g_handoff_by_slot[SLOT_COUNT];
static volatile uint32_t g_lock_by_slot[SLOT_COUNT];      /* t_lock  */
static volatile uint32_t g_post_by_slot[SLOT_COUNT];      /* t_post  */
static volatile uint32_t g_round_by_slot[SLOT_COUNT];     /* t_round */
static volatile uint32_t g_bcast_by_slot[SLOT_COUNT];     /* t_bcast or 0 */

/*
 * t_round measurement is genuinely cross-thread: the actuator writes
 * a timestamp into a shared atomic, the controller reads it after
 * its sem-pend wake. SCB.CYCCNT is the same monotonic source used in
 * the ISR, so subtraction is meaningful.
 */
static volatile uint32_t g_actuator_done_cyc = 0U;
static volatile uint16_t g_actuator_done_seq = 0U;

/*
 * Telemetry integrity counter — design doc Section "Risks" calls
 * for an assert that the telemetry thread runs at all. Incremented
 * in the telemetry loop; printed in the footer.
 */
static volatile uint32_t g_telemetry_emits = 0U;

/* Latest fusion update tag — the controller, on its 100 Hz tick,
 * pairs (t_lock, t_post, t_round) onto whatever the most recent
 * sensor seq is. Lets us emit a single-row CSV per sensor sample
 * with -1 for unmeasured columns. */
static volatile uint16_t g_last_ctrl_seq = 0U;
static volatile bool     g_ctrl_measurement_pending = false;

/* ------------------------------------------------------- primitives */

/*
 * Two rings on the sensor path:
 *   sample_ring     — sensor ISR -> fusion thread (the "primitive
 *                     under test" handoff: ring_buf + sem give-from-isr)
 *   emit_ring       — fusion thread -> reader thread (off-target event
 *                     dispatch; not part of the measured path, just
 *                     plumbing so the CSV emit doesn't compete with
 *                     fusion for the data_ready sem)
 *
 * The two-ring split avoids the single-sem bug where reader_loop and
 * fusion both pend on `sensor_data_ready` and steal samples from each
 * other (caught in initial QEMU smoke).
 */
RING_BUF_DECLARE(sample_ring, RING_CAPACITY_SAMPLES * sizeof(struct imu_sample));
RING_BUF_DECLARE(emit_ring,   RING_CAPACITY_SAMPLES * sizeof(struct imu_sample));

static struct k_timer sensor_timer;
static struct k_timer ctrl_timer;
static struct k_sem   sensor_data_ready;   /* ISR -> fusion */
static struct k_sem   emit_ready;          /* fusion -> reader */
static struct k_sem   ctrl_tick;
static struct k_sem   actuator_done[3];

K_MUTEX_DEFINE(state_mutex);
K_CONDVAR_DEFINE(telemetry_cv);
K_MSGQ_DEFINE(actuator_q, ACTUATOR_MSG_BYTES, ACTUATOR_QUEUE_DEPTH, 4);

static struct flight_state g_state;   /* protected by state_mutex */

/* ---------------------------------------------------------------- ISR */

static void sensor_isr(struct k_timer *t)
{
	ARG_UNUSED(t);

	uint32_t t_entry = k_cycle_get_32();

	struct imu_sample s = {
		.gyro_x  = g_imu_gyro_x,
		.gyro_y  = g_imu_gyro_y,
		.gyro_z  = g_imu_gyro_z,
		.accel_x = g_imu_accel_x,
		.accel_y = g_imu_accel_y,
		.accel_z = g_imu_accel_z,
	};

	/* algo segment — small filter precomputation. The actual filter
	 * runs in the fusion thread under the mutex; we keep the ISR
	 * algo segment small but non-trivial so it's measurable. Same
	 * shape as engine_control's control_step in the ISR. */
	int32_t squelch = (int32_t)s.gyro_x + s.gyro_y + s.gyro_z;
	if (squelch < 0) squelch = -squelch;
	s.algo_cycles = (uint32_t)squelch & 0xFFFFu;   /* placeholder; below */

	uint32_t t_algo_end = k_cycle_get_32();

	uint16_t seq_snapshot = g_seq;
	uint32_t algo_cycles  = t_algo_end - t_entry;

	s.algo_cycles = algo_cycles;
	s.seq         = seq_snapshot;
	s.step        = g_step;

	uint32_t wrote = ring_buf_put(&sample_ring, (uint8_t *)&s, sizeof(s));
	if (wrote != sizeof(s)) {
		g_drops++;
	} else {
		k_sem_give(&sensor_data_ready);
	}

	uint32_t t_exit = k_cycle_get_32();
	uint32_t handoff_cycles = t_exit - t_algo_end;

	g_handoff_by_slot[seq_snapshot % SLOT_COUNT] = handoff_cycles;
	/* Initialize cross-thread slots to 0; emit_event reports them
	 * as -1 if no controller cycle has tagged them by the time
	 * the reader processes the sample. (The matching seq might
	 * trail by up to ~10 sensor ticks at 1 kHz / 100 Hz.) */
	g_lock_by_slot [seq_snapshot % SLOT_COUNT] = 0U;
	g_post_by_slot [seq_snapshot % SLOT_COUNT] = 0U;
	g_round_by_slot[seq_snapshot % SLOT_COUNT] = 0U;
	g_bcast_by_slot[seq_snapshot % SLOT_COUNT] = 0U;

	g_seq++;
	g_sensor_ints++;

	if (g_fire_budget > 0U) {
		g_fire_budget--;
		if (g_fire_budget == 0U) {
			k_timer_stop(&sensor_timer);
		}
	}
}

static void ctrl_isr(struct k_timer *t)
{
	ARG_UNUSED(t);
	/* Just wake the controller thread — no work in the ISR itself.
	 * Same shape as a typical RTOS rate-group dispatch. */
	k_sem_give(&ctrl_tick);
}

/* ---------------------------------------------------------- threads */

/*
 * Fusion thread (priority 4 — second-highest, only the ISR preempts
 * it). Drains the sensor ring under k_sem, runs filter_step under
 * the contended state mutex, and broadcasts the telemetry condvar
 * every CONDVAR_BROADCAST_EVERY-th update.
 */
static void fusion_loop(void *a, void *b, void *c)
{
	ARG_UNUSED(a); ARG_UNUSED(b); ARG_UNUSED(c);

	uint32_t cycle = 0;
	for (;;) {
		k_sem_take(&sensor_data_ready, K_FOREVER);
		struct imu_sample s;
		while (ring_buf_get(&sample_ring, (uint8_t *)&s, sizeof(s))
		       == sizeof(s)) {
			/* Critical section: filter + state update. The
			 * controller, telemetry, and noise threads all
			 * contend for this mutex — it's the focus of the
			 * t_lock measurement (timed by the controller, see
			 * ctrl_loop()). */
			k_mutex_lock(&state_mutex, K_FOREVER);
			filter_step(&g_state, &s);
			g_state.updates++;
			k_mutex_unlock(&state_mutex);

			cycle++;
			if ((cycle % CONDVAR_BROADCAST_EVERY) == 0U) {
				/* t_bcast: enter→exit of the broadcast.
				 * The matching wake on the telemetry side
				 * is observed when the telemetry thread
				 * runs; we publish elapsed-here only. */
				uint32_t t0 = k_cycle_get_32();
				k_mutex_lock(&state_mutex, K_FOREVER);
				k_condvar_broadcast(&telemetry_cv);
				k_mutex_unlock(&state_mutex);
				uint32_t t1 = k_cycle_get_32();
				/* Tag the broadcast onto the matching
				 * sensor seq's slot. */
				g_bcast_by_slot[s.seq % SLOT_COUNT] = t1 - t0;
			}

			/* Republish the sample on the emit ring so the
			 * reader can pick it up without competing with us
			 * on sensor_data_ready. */
			uint32_t wrote = ring_buf_put(&emit_ring,
						      (uint8_t *)&s, sizeof(s));
			if (wrote == sizeof(s)) {
				k_sem_give(&emit_ready);
			}
		}
	}
}

/*
 * Controller thread (priority 5) — runs at 100 Hz off ctrl_timer.
 * Locks the state mutex (t_lock), copies state, posts a command to
 * the actuator msgq (t_post), then waits for the first actuator's
 * sem_give and reads the cross-thread atomic to compute t_round.
 * Tags the most recent sensor seq's slot.
 */
static void ctrl_loop(void *a, void *b, void *c)
{
	ARG_UNUSED(a); ARG_UNUSED(b); ARG_UNUSED(c);

	uint8_t msgbuf[ACTUATOR_MSG_BYTES];

	for (;;) {
		k_sem_take(&ctrl_tick, K_FOREVER);

		/* Sample-pair the measurement onto the most recent sensor
		 * seq. By the time the controller wakes, fusion has
		 * processed the sensor sample and incremented g_state.updates;
		 * we use that as the slot key (mod SLOT_COUNT) so the row
		 * matching the latest event gets t_lock/t_post/t_round.
		 * Note: g_seq is the next-to-be-written; subtract 1 for
		 * the most recent published. */
		uint16_t seq_tag = (uint16_t)(g_seq - 1U);

		uint32_t t_lock_in = k_cycle_get_32();
		k_mutex_lock(&state_mutex, K_FOREVER);
		struct flight_state snap = g_state;
		k_mutex_unlock(&state_mutex);
		uint32_t t_lock_out = k_cycle_get_32();
		uint32_t t_lock = t_lock_out - t_lock_in;

		uint32_t cmd = controller_step(&snap);
		(void)cmd;

		/* Build payload of the configured size. Lower bytes hold
		 * the actual command; upper bytes are filler so the msgq
		 * copy cost scales with g_payload. */
		memset(msgbuf, 0, sizeof(msgbuf));
		msgbuf[0] = (uint8_t)(cmd >>  0);
		msgbuf[1] = (uint8_t)(cmd >>  8);
		msgbuf[2] = (uint8_t)(cmd >> 16);
		msgbuf[3] = (uint8_t)(cmd >> 24);
		uint16_t my_seq = (uint16_t)g_state.updates;
		msgbuf[4] = (uint8_t)(my_seq >> 0);
		msgbuf[5] = (uint8_t)(my_seq >> 8);

		uint32_t t_post_in = k_cycle_get_32();
		(void)k_msgq_put(&actuator_q, msgbuf, K_NO_WAIT);
		uint32_t t_post_out = k_cycle_get_32();
		uint32_t t_post = t_post_out - t_post_in;

		/* Wait for the first actuator to complete. K_MSEC(2) ~=
		 * 200 ticks at 100 kHz tick — generous; in practice
		 * actuators wake within a few hundred cycles. */
		uint32_t t_round = 0U;
		if (k_sem_take(&actuator_done[0], K_MSEC(2)) == 0) {
			uint32_t done_cyc = g_actuator_done_cyc;
			t_round = done_cyc - t_post_out;
		}

		uint32_t slot = seq_tag % SLOT_COUNT;
		g_lock_by_slot [slot] = t_lock;
		g_post_by_slot [slot] = t_post;
		g_round_by_slot[slot] = t_round;
		g_last_ctrl_seq = seq_tag;
	}
}

/*
 * Actuator thread template. Three instances run at priorities 6/7/8;
 * each waits on the same msgq. Only actuator 0 publishes the round-trip
 * timestamp (t_round is one number per controller post; multiplexing
 * across all three would muddle the measurement).
 */
struct actuator_ctx {
	uint8_t  id;
	uint16_t cycles_busy;   /* tunable busy-wait, simulates actuator work */
};

static void actuator_loop(void *a_, void *b, void *c)
{
	ARG_UNUSED(b); ARG_UNUSED(c);
	struct actuator_ctx *ctx = a_;

	uint8_t msgbuf[ACTUATOR_MSG_BYTES];
	for (;;) {
		k_msgq_get(&actuator_q, msgbuf, K_FOREVER);

		/* Simulated actuator work — fixed-cycle busy loop, no
		 * I/O. Bounded execution per the design doc. */
		uint32_t end = k_cycle_get_32() + ctx->cycles_busy;
		while ((int32_t)(end - k_cycle_get_32()) > 0) {
			__asm__ volatile ("nop");
		}

		if (ctx->id == 0U) {
			/* t_round publish: write the timestamp BEFORE the
			 * sem_give so the controller's post-wake read sees
			 * a value at least as old as the wake. The 32-bit
			 * cycle counter is the same monotonic source. */
			g_actuator_done_cyc = k_cycle_get_32();
			g_actuator_done_seq++;
			k_sem_give(&actuator_done[0]);
		} else {
			k_sem_give(&actuator_done[ctx->id]);
		}
	}
}

/*
 * Telemetry thread (priority 9 — lowest). Sits on the condvar and
 * reads state under the same mutex. Priority-inheritance is the
 * feature being measured: when fusion holds the mutex, the kernel
 * boosts it briefly so telemetry isn't starved by mid-prio actuators.
 *
 * Per design doc Section "Risks": this thread MUST run at least
 * once. We track g_telemetry_emits as the integrity counter.
 */
static void telemetry_loop(void *a, void *b, void *c)
{
	ARG_UNUSED(a); ARG_UNUSED(b); ARG_UNUSED(c);

	for (;;) {
		k_mutex_lock(&state_mutex, K_FOREVER);
		k_condvar_wait(&telemetry_cv, &state_mutex, K_FOREVER);
		struct flight_state snap = g_state;
		k_mutex_unlock(&state_mutex);
		(void)snap;
		g_telemetry_emits++;
	}
}

/*
 * Mutex-noise thread. Runs at priority 6 (above telemetry, below
 * actuators); spins lock-unlock to add contention pressure. Number
 * of running noise threads = g_contention (0/1/2). They all sit on
 * the same `noise_run` semaphore which is given by the driver
 * before each step, taken back at the end. */
static struct k_sem noise_run[2];

static void noise_loop(void *a_, void *b, void *c)
{
	ARG_UNUSED(b); ARG_UNUSED(c);
	uint32_t id = (uint32_t)(uintptr_t)a_;
	for (;;) {
		k_sem_take(&noise_run[id], K_FOREVER);
		while (g_contention > id) {
			k_mutex_lock(&state_mutex, K_FOREVER);
			/* tiny work — keep contention real, not a tight
			 * bus-eater that starves higher-prio threads */
			for (volatile int i = 0; i < 50; i++) { }
			k_mutex_unlock(&state_mutex);
			k_yield();
		}
	}
}

/* ---------------------------------------------------------- reporter */

static uint32_t reader_count = 0;
static uint32_t reader_skipped = 0;   /* sensor-rate rows without ctrl tag */

/*
 * Returns true if a row was emitted, false if skipped.
 *
 * Skip rule: rows whose slot has no controller-cycle pair-tag
 * (t_lock == 0) are dropped from the CSV. Without this, the bench
 * emits one row per sensor ISR (~1 kHz), of which only ~10 % carry
 * the marquee t_lock/t_post/t_round measurements (controller runs
 * at 100 Hz). The 90 % near-empty rows starved Renode at the UART
 * — run 25135494876 ran out of CI budget after 13 of 27 cells.
 *
 * This change keeps the bench's ISR-side measurements (algo /
 * handoff) honest by emitting them only on ctrl-tagged rows
 * (where they are still valid), and drops the dilute fraction.
 * Per-cell N drops from "150 sensor rows, 8 ctrl-measured" to
 * "100 ctrl-measured rows, every column populated."
 */
static bool emit_event(const struct imu_sample *s)
{
	uint32_t slot = s->seq % SLOT_COUNT;
	uint32_t handoff = g_handoff_by_slot[slot];
	uint32_t t_lock  = g_lock_by_slot [slot];
	uint32_t t_post  = g_post_by_slot [slot];
	uint32_t t_round = g_round_by_slot[slot];
	uint32_t t_bcast = g_bcast_by_slot[slot];

	if (t_lock == 0U) {
		return false;
	}

	int32_t f_lock  = (int32_t)t_lock;
	int32_t f_post  = (t_post  == 0U) ? -1 : (int32_t)t_post;
	int32_t f_round = (t_round == 0U) ? -1 : (int32_t)t_round;
	int32_t f_bcast = (t_bcast == 0U) ? -1 : (int32_t)t_bcast;

	printf("E,%u,%u,%u,%u,%u,%d,%d,%d,%d\n",
	       (unsigned)s->seq, (unsigned)s->step, (unsigned)g_load,
	       (unsigned)s->algo_cycles, (unsigned)handoff,
	       (int)f_lock, (int)f_post, (int)f_round, (int)f_bcast);
	return true;
}

static void reader_loop(void)
{
	while (reader_count < TOTAL_SAMPLES) {
		k_sem_take(&emit_ready, K_FOREVER);
		struct imu_sample s;
		while (ring_buf_get(&emit_ring, (uint8_t *)&s, sizeof(s))
		       == sizeof(s)) {
			if (emit_event(&s)) {
				reader_count++;
			} else {
				reader_skipped++;
			}
			if (reader_count >= TOTAL_SAMPLES) break;
		}
	}
}

static void print_csv_header(void)
{
	uint32_t hz = sys_clock_hw_cycles_per_sec();
	printf("=== FLIGHT-CONTROL BENCH RESULTS ===\n");
	printf("build,%s\n",
#ifdef CONFIG_GALE_KERNEL_SEM
	       "gale"
#else
	       "baseline"
#endif
	);
	printf("cycles_per_sec,%u\n", hz);
	printf("target_samples,%u\n", TOTAL_SAMPLES);
	printf("# event rows: E,<seq>,<step>,<load>,<algo>,<handoff>,"
	       "<t_lock>,<t_post>,<t_round>,<t_bcast>\n");
}

static void print_csv_footer(void)
{
	printf("drops,%u\n", g_drops);
	printf("samples,%u\n", reader_count);
	printf("samples_skipped,%u\n", reader_skipped);
	printf("telemetry_emits,%u\n", g_telemetry_emits);
	printf("=== END ===\n");
}

/* -------------------------------------------------------- driver */

/*
 * Drive simulated IMU values for visual diversity in the log + to keep
 * the filter doing real work. Pure-integer state machine.
 */
static void update_imu_inputs(uint16_t hz)
{
	static uint32_t phase = 0U;
	phase += hz / 100U;
	g_imu_gyro_x = (int16_t)((phase * 17U) & 0x3FF) - 512;
	g_imu_gyro_y = (int16_t)((phase * 23U) & 0x3FF) - 512;
	g_imu_gyro_z = (int16_t)((phase * 11U) & 0x1FF) - 256;
	g_imu_accel_x = (int16_t)((phase * 7U) & 0x1FF) - 256;
	g_imu_accel_y = (int16_t)((phase * 13U) & 0x1FF) - 256;
	g_imu_accel_z = 1000;
}

static void sweep_driver(void *a, void *b, void *c)
{
	ARG_UNUSED(a); ARG_UNUSED(b); ARG_UNUSED(c);

	for (size_t i = 0; i < SWEEP_STEPS; i++) {
		const struct sweep_step *st = &sweep[i];
		g_step       = (uint8_t)i;
		g_sensor_hz  = st->sensor_hz;
		g_contention = st->contention;
		g_payload    = st->payload;
		g_load       = ((uint16_t)st->sensor_hz)
			     | ((uint16_t)st->contention << 13);

		/* Wake the noise threads needed for this step. The threads
		 * loop on `g_contention > id` so they self-stop when the
		 * driver lowers g_contention before the next sem give. */
		for (uint8_t n = 0; n < 2; n++) {
			if (n < st->contention) {
				k_sem_give(&noise_run[n]);
			}
		}

		uint32_t period_us = hz_to_period_us(st->sensor_hz);
		if (period_us == 0) period_us = 1;

		update_imu_inputs(st->sensor_hz);

		uint32_t start_emits = reader_count;
		uint32_t start_ints  = g_sensor_ints;
		g_fire_budget = st->samples;
		k_timer_start(&sensor_timer,
			      K_USEC(period_us),
			      K_USEC(period_us));

		uint32_t budget_ms = st->samples * period_us / 1000U + 2000U;
		uint32_t t0 = k_uptime_get_32();
		bool budget_exceeded = false;
		while (g_fire_budget > 0U) {
			if ((k_uptime_get_32() - t0) > budget_ms) {
				budget_exceeded = true;
				break;
			}
			k_msleep(5);
		}
		k_timer_stop(&sensor_timer);

		/*
		 * Drain in controller-rate units. The reader emits only
		 * rows tagged by ctrl_loop (see emit_event). Expected
		 * ctrl-rows ≈ sensor_budget * (100 Hz ctrl / sensor_hz).
		 * Drain timeout is short (5s) because the cell already
		 * waited budget_ms above for sensor ISRs to retire.
		 */
		uint32_t expected_ctrl =
			(uint32_t)st->samples * 100U / st->sensor_hz;
		uint32_t drain_target = start_emits + expected_ctrl;
		uint32_t drain_t0 = k_uptime_get_32();
		bool drain_timeout = false;
		while (reader_count < drain_target) {
			if ((k_uptime_get_32() - drain_t0) > 5000U) {
				drain_timeout = true;
				break;
			}
			k_msleep(5);
		}

		printf("# step %u/%u sensor_hz=%u contention=%u payload=%u "
		       "ints=%u expected=%u count=%u%s%s\n",
		       (unsigned)(i + 1), (unsigned)SWEEP_STEPS,
		       (unsigned)st->sensor_hz, (unsigned)st->contention,
		       (unsigned)st->payload,
		       (unsigned)(g_sensor_ints - start_ints),
		       (unsigned)expected_ctrl,
		       (unsigned)(reader_count - start_emits),
		       budget_exceeded ? " [budget_exceeded]" : "",
		       drain_timeout   ? " [drain_timeout]"   : "");

		/* Lower contention before the next step so noise threads
		 * exit their inner loop and re-pend on the sem. */
		g_contention = 0U;
	}

	uint32_t final_t0 = k_uptime_get_32();
	while (reader_count < TOTAL_SAMPLES && reader_count < g_sensor_ints) {
		if ((k_uptime_get_32() - final_t0) > 30000U) {
			printk("final drain timeout; count=%u\n",
			       (unsigned)reader_count);
			break;
		}
		k_msleep(5);
	}
}

/* ---------------------------------------------------------------- main */

static K_THREAD_STACK_DEFINE(fusion_stack,    2048);
static K_THREAD_STACK_DEFINE(ctrl_stack,      2048);
static K_THREAD_STACK_DEFINE(act_stack_0,     1024);
static K_THREAD_STACK_DEFINE(act_stack_1,     1024);
static K_THREAD_STACK_DEFINE(act_stack_2,     1024);
static K_THREAD_STACK_DEFINE(tele_stack,      1024);
static K_THREAD_STACK_DEFINE(noise_stack_0,    768);
static K_THREAD_STACK_DEFINE(noise_stack_1,    768);
static K_THREAD_STACK_DEFINE(sweep_stack,     1536);

static struct k_thread fusion_th, ctrl_th, act_th[3], tele_th, noise_th[2], sweep_th;

static struct actuator_ctx act_ctx[3] = {
	{ .id = 0, .cycles_busy = 100 },
	{ .id = 1, .cycles_busy = 120 },
	{ .id = 2, .cycles_busy = 140 },
};

int main(void)
{
	k_sem_init(&sensor_data_ready, 0, K_SEM_MAX_LIMIT);
	k_sem_init(&emit_ready,        0, K_SEM_MAX_LIMIT);
	k_sem_init(&ctrl_tick,         0, K_SEM_MAX_LIMIT);
	for (int i = 0; i < 3; i++) {
		k_sem_init(&actuator_done[i], 0, K_SEM_MAX_LIMIT);
	}
	k_sem_init(&noise_run[0], 0, K_SEM_MAX_LIMIT);
	k_sem_init(&noise_run[1], 0, K_SEM_MAX_LIMIT);

	k_timer_init(&sensor_timer, sensor_isr, NULL);
	k_timer_init(&ctrl_timer,   ctrl_isr,   NULL);

	printk("flight_control bench starting "
	       "(target %u samples, %u sweep steps)\n",
	       TOTAL_SAMPLES, (unsigned)SWEEP_STEPS);

	/* Run main (== reader_loop) at priority 10 — below ALL worker
	 * threads so reader_loop's printf back-pressure cannot starve
	 * the controller / actuators. Telemetry stays at 9 (lowest of
	 * the design's worker threads) so it still wakes on the
	 * condvar; main reads emit_ring and burns UART, doesn't compete
	 * for the state mutex. */
	k_thread_priority_set(k_current_get(), 10);

	print_csv_header();

	k_thread_create(&fusion_th, fusion_stack, K_THREAD_STACK_SIZEOF(fusion_stack),
			fusion_loop, NULL, NULL, NULL,
			4, 0, K_NO_WAIT);
	k_thread_name_set(&fusion_th, "fusion");

	k_thread_create(&ctrl_th, ctrl_stack, K_THREAD_STACK_SIZEOF(ctrl_stack),
			ctrl_loop, NULL, NULL, NULL,
			5, 0, K_NO_WAIT);
	k_thread_name_set(&ctrl_th, "ctrl");

	k_thread_create(&act_th[0], act_stack_0, K_THREAD_STACK_SIZEOF(act_stack_0),
			actuator_loop, &act_ctx[0], NULL, NULL,
			6, 0, K_NO_WAIT);
	k_thread_name_set(&act_th[0], "act0");

	k_thread_create(&act_th[1], act_stack_1, K_THREAD_STACK_SIZEOF(act_stack_1),
			actuator_loop, &act_ctx[1], NULL, NULL,
			7, 0, K_NO_WAIT);
	k_thread_name_set(&act_th[1], "act1");

	k_thread_create(&act_th[2], act_stack_2, K_THREAD_STACK_SIZEOF(act_stack_2),
			actuator_loop, &act_ctx[2], NULL, NULL,
			8, 0, K_NO_WAIT);
	k_thread_name_set(&act_th[2], "act2");

	k_thread_create(&tele_th, tele_stack, K_THREAD_STACK_SIZEOF(tele_stack),
			telemetry_loop, NULL, NULL, NULL,
			9, 0, K_NO_WAIT);
	k_thread_name_set(&tele_th, "tele");

	k_thread_create(&noise_th[0], noise_stack_0, K_THREAD_STACK_SIZEOF(noise_stack_0),
			noise_loop, (void *)(uintptr_t)0, NULL, NULL,
			6, 0, K_NO_WAIT);
	k_thread_name_set(&noise_th[0], "noise0");

	k_thread_create(&noise_th[1], noise_stack_1, K_THREAD_STACK_SIZEOF(noise_stack_1),
			noise_loop, (void *)(uintptr_t)1, NULL, NULL,
			6, 0, K_NO_WAIT);
	k_thread_name_set(&noise_th[1], "noise1");

	/* Start the controller-rate timer at 100 Hz. The driver toggles
	 * the sensor timer per-step. */
	k_timer_start(&ctrl_timer, K_USEC(10000), K_USEC(10000));

	k_thread_create(&sweep_th, sweep_stack, K_THREAD_STACK_SIZEOF(sweep_stack),
			sweep_driver, NULL, NULL, NULL,
			4, 0, K_NO_WAIT);
	k_thread_name_set(&sweep_th, "sweep");

	reader_loop();
	k_timer_stop(&sensor_timer);
	k_timer_stop(&ctrl_timer);
	print_csv_footer();
	return 0;
}
