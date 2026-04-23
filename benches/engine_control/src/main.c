/*
 * SPDX-License-Identifier: Apache-2.0
 * Copyright (c) 2026 PulseEngine
 *
 * Engine-control interrupt benchmark — main app (event-stream edition).
 *
 * A k_timer callback (ISR context) simulates a crank-position sensor.
 * Each firing runs a pure-C engine-control algorithm (table lookups
 * for spark advance + fuel duration), then exercises the Zephyr
 * primitive chain Gale replaces (ring_buf_put + k_sem_give).
 *
 * Two timings are recorded per interrupt:
 *
 *   algo_cycles    = t_algo_end - t_entry   # pure C algorithm
 *   handoff_cycles = t_exit - t_algo_end    # ring_buf + sem_give
 *
 * Methodology (post-#25):
 *   The ISR no longer computes statistics. It records the two raw
 *   cycle deltas per sample and the sweep-step index, then the
 *   reader thread emits one CSV event line per sample to UART:
 *
 *     E,<seq>,<step>,<rpm>,<algo_cycles>,<handoff_cycles>
 *
 *   Off-target analyzer (analyze.py) groups events by RPM step and
 *   computes median + bootstrap 95% CI + Mann-Whitney U p-value on
 *   the per-step distributions. No on-target mean/min/max/histogram.
 *
 * Why: the in-ISR histogram approach had a mean-divisor mismatch
 * (histogram totals diverged from reader `count`) and log-scale
 * buckets masked sub-bucket shifts. Emitting raw events puts all
 * statistics off the target where they can be recomputed and audited.
 * See docs/research/engine-bench-methodology-review.md and #25.
 */

#include <zephyr/kernel.h>
#include <zephyr/sys/ring_buffer.h>
#include <stdbool.h>
#include <stdio.h>
#include <string.h>

#include "control.h"

/* ------------------------------------------------------------- knobs */

#define RING_CAPACITY_SAMPLES  256

struct sweep_step {
	uint32_t rpm;
	uint32_t samples;
};

#if defined(ENGINE_BENCH_SWEEP_long)
/* Renode / real-hardware long run — more RPM stations, more samples
 * each, captures the full idle→redline curve with statistical weight. */
static const struct sweep_step sweep[] = {
	{  500, 500 },   { 1000, 500 },   { 1500, 500 },
	{ 2000, 1000 },  { 2500, 1000 },  { 3000, 1000 },
	{ 4000, 1000 },  { 5000, 1000 },  { 6000,  500 },
	{ 7000,  300 },  { 8000,  200 },  { 9000,  150 },
	{10000,  100 },
};
#else
/* QEMU smoke / short interactive run. */
static const struct sweep_step sweep[] = {
	{ 1000, 30 },
	{ 2000, 30 },
	{ 4000, 30 },
	{ 6000, 30 },
	{10000, 30 },
};
#endif
#define SWEEP_STEPS (sizeof(sweep) / sizeof(sweep[0]))

/* TOTAL_SAMPLES must equal sum(sweep[].samples) so every step runs to
 * completion. Computed at compile time from the sweep array to avoid
 * the audit-flagged bug where TOTAL_SAMPLES=150 truncated the short
 * sweep at step 2 of 5 (RPM 1000 + 2000 only). If you override via
 * -DENGINE_BENCH_TOTAL_SAMPLES, make sure it matches sum(sweep[].samples)
 * or accept that the sweep may terminate early. */
#if defined(ENGINE_BENCH_SWEEP_long)
#  ifndef TOTAL_SAMPLES
#    define TOTAL_SAMPLES  7750u  /* sum of long sweep above */
#  endif
#else
#  ifndef TOTAL_SAMPLES
#    define TOTAL_SAMPLES  150u   /* 5 steps × 30 samples */
#  endif
#endif

/* For a 4-stroke engine with one interrupt per crank degree:
 *   period_us = 60_000_000 / (rpm * 360) = 166_667 / rpm. */
static inline uint32_t rpm_to_period_us(uint32_t rpm)
{
	if (rpm == 0) {
		return 1000000U;
	}
	return 166667U / rpm;
}

/* ------------------------------------------------------------- state */

static struct k_timer crank_timer;
static struct k_sem   data_ready;

RING_BUF_DECLARE(sample_ring, RING_CAPACITY_SAMPLES * sizeof(struct crank_sample));

static volatile uint32_t g_rpm      = 500U;
static volatile uint8_t  g_step     = 0U;    /* sweep index, written by driver */
static volatile uint16_t g_load_pct = 45U;
static volatile int16_t  g_coolant  = 85;
static volatile uint8_t  g_knock    = 0;

static volatile uint32_t g_angle      = 0U;
static volatile uint16_t g_seq        = 0U;
static volatile uint16_t g_drops      = 0U;
static volatile uint32_t g_interrupts = 0U;

/* Per-step "fire this many times then stop" limit. Written by the
 * sweep driver before each step, decremented by the ISR. When it
 * hits zero the ISR stops the timer so overshoot is bounded to the
 * one extra shot that may have queued. Without this, high-RPM steps
 * would fire hundreds of times before the driver's k_msleep(1) poll
 * could notice the sample count had been reached. */
static volatile uint32_t g_fire_budget = 0U;

/* Side-channel array for handoff_cycles. The ISR measures t_exit AFTER
 * ring_buf_put (which is the point — handoff cost includes the put +
 * sem_give). But by then the sample bytes are already in the ring, so
 * we can't write handoff_cycles into the in-ring sample. We publish
 * the measurement here, keyed by seq mod RING_CAPACITY_SAMPLES. SPSC
 * ordering: the ISR always writes this slot before the reader can
 * observe the matching ring entry (reader wakes on k_sem_give, which
 * happens BEFORE t_exit is read). */
static volatile uint32_t g_handoff_by_slot[RING_CAPACITY_SAMPLES];

/* ---------------------------------------------------------------- ISR */

static void crank_isr(struct k_timer *t)
{
	ARG_UNUSED(t);

	uint32_t t_entry = k_cycle_get_32();

	struct engine_state in = {
		.rpm          = g_rpm,
		.load_pct     = g_load_pct,
		.coolant_c    = g_coolant,
		.knock_retard = g_knock,
	};
	int16_t spark;
	uint16_t fuel;
	control_step(&in, &spark, &fuel);

	uint32_t t_algo_end = k_cycle_get_32();

	uint32_t seq_snapshot = g_seq;
	uint32_t algo_cycles = t_algo_end - t_entry;

	struct crank_sample s = {
		.angle_deg         = g_angle,
		.rpm               = in.rpm,
		.algo_cycles       = algo_cycles,
		.spark_advance_deg = spark,
		.fuel_duration_us  = fuel,
		.drops_so_far      = g_drops,
		.seq               = (uint16_t)seq_snapshot,
		.step              = g_step,
	};

	uint32_t wrote = ring_buf_put(&sample_ring, (uint8_t *)&s, sizeof(s));
	if (wrote != sizeof(s)) {
		g_drops++;
	} else {
		k_sem_give(&data_ready);
	}

	uint32_t t_exit = k_cycle_get_32();
	uint32_t handoff_cycles = t_exit - t_algo_end;

	/* Publish handoff into a side channel indexed by seq. The ring
	 * buffer is SPSC: the consumer sees a sample only after this
	 * ISR completes (it ran to k_sem_give already but the consumer
	 * thread can't preempt ISR), so writing the slot here is still
	 * before the consumer observes the matching sample. */
	g_handoff_by_slot[seq_snapshot % RING_CAPACITY_SAMPLES] = handoff_cycles;

	g_seq++;
	g_angle = (g_angle + 1U) % 720U;
	g_interrupts++;

	if (g_fire_budget > 0U) {
		g_fire_budget--;
		if (g_fire_budget == 0U) {
			/* Stop the timer from ISR context: k_timer_stop is
			 * ISR-safe per Zephyr docs. This caps per-step
			 * overshoot to 1 extra firing (the one already in
			 * flight when we hit zero). */
			k_timer_stop(&crank_timer);
		}
	}
}

/* ---------------------------------------------------------- reporter */

static uint32_t count = 0;

static void emit_event(const struct crank_sample *s)
{
	uint32_t handoff = g_handoff_by_slot[s->seq % RING_CAPACITY_SAMPLES];
	printf("E,%u,%u,%u,%u,%u\n",
	       (unsigned)s->seq, (unsigned)s->step, (unsigned)s->rpm,
	       (unsigned)s->algo_cycles, (unsigned)handoff);
}

static void reader_loop(void)
{
	while (count < TOTAL_SAMPLES) {
		k_sem_take(&data_ready, K_FOREVER);
		struct crank_sample s;
		while (ring_buf_get(&sample_ring, (uint8_t *)&s, sizeof(s))
		       == sizeof(s)) {
			emit_event(&s);
			count++;
			if (count >= TOTAL_SAMPLES) break;
		}
	}
}

static void print_csv_header(void)
{
	uint32_t hz = sys_clock_hw_cycles_per_sec();
	printf("=== ENGINE-CONTROL BENCH RESULTS ===\n");
	printf("build,%s\n",
#ifdef CONFIG_GALE_KERNEL_SEM
	       "gale"
#else
	       "baseline"
#endif
	);
	printf("cycles_per_sec,%u\n", hz);
	printf("target_samples,%u\n", TOTAL_SAMPLES);
	printf("# event rows: E,<seq>,<step>,<rpm>,<algo_cycles>,<handoff_cycles>\n");
}

static void print_csv_footer(void)
{
	printf("drops,%u\n", g_drops);
	printf("samples,%u\n", count);
	printf("=== END ===\n");
}

/* ------------------------------------------------------------- driver */

static void sweep_driver(void)
{
	for (size_t i = 0; i < SWEEP_STEPS; i++) {
		g_step = (uint8_t)i;
		g_rpm = sweep[i].rpm;
		uint32_t period_us = rpm_to_period_us(g_rpm);
		if (period_us == 0) period_us = 1;

		/* Set the ISR's fire-budget BEFORE starting the timer.
		 * The ISR will self-stop after exactly sweep[i].samples
		 * firings (bounded-overshoot design — cf. the audit which
		 * flagged polling-based termination). */
		uint32_t start_ints = g_interrupts;
		g_fire_budget = sweep[i].samples;
		k_timer_start(&crank_timer,
			      K_USEC(period_us),
			      K_USEC(period_us));

		/* Wait for the ISR to self-stop. Budget = theoretical
		 * duration + 2s floor for UART back-pressure that may
		 * stretch the observed period. */
		uint32_t budget_ms = sweep[i].samples * period_us / 1000U
				     + 2000U;
		uint32_t t0 = k_uptime_get_32();
		bool budget_exceeded = false;
		while (g_fire_budget > 0U) {
			if ((k_uptime_get_32() - t0) > budget_ms) {
				budget_exceeded = true;
				break;
			}
			k_msleep(5);
		}
		/* Belt-and-suspenders: if we exited via the budget, the ISR
		 * may still be armed; stop it here. k_timer_stop is
		 * idempotent. */
		k_timer_stop(&crank_timer);

		/* Drain between steps — the reader emits events over UART
		 * which is the slow path. Without this back-pressure the
		 * ring fills and subsequent ISRs drop samples. We wait for
		 * the reader to catch up (or give up after a generous
		 * ceiling so a stuck UART doesn't hang CI forever). */
		uint32_t drain_t0 = k_uptime_get_32();
		uint32_t target = start_ints + sweep[i].samples;
		bool drain_timeout = false;
		while (count < target && count < g_interrupts) {
			if ((k_uptime_get_32() - drain_t0) > 30000U) {
				drain_timeout = true;
				break;
			}
			k_msleep(5);
		}

		/* Now safe to emit step-completion marker: no ISR firing,
		 * reader has drained, so no interleaving with event lines.
		 * Using '#' prefix so analyze.py ignores these as comments. */
		printf("# step %u/%u rpm=%u period_us=%u ints=%u count=%u%s%s\n",
		       (unsigned)(i + 1), (unsigned)SWEEP_STEPS,
		       (unsigned)g_rpm, (unsigned)period_us,
		       (unsigned)(g_interrupts - start_ints),
		       (unsigned)count,
		       budget_exceeded ? " [budget_exceeded]" : "",
		       drain_timeout   ? " [drain_timeout]"   : "");
	}
	/* Final drain. Cap at g_interrupts so we don't hang forever if a
	 * step hit its budget and produced fewer than planned samples. */
	uint32_t final_t0 = k_uptime_get_32();
	while (count < TOTAL_SAMPLES && count < g_interrupts) {
		if ((k_uptime_get_32() - final_t0) > 30000U) {
			printk("final drain timeout; count=%u\n", (unsigned)count);
			break;
		}
		k_msleep(5);
	}
}

/* -------------------------------------------------------------- main */

int main(void)
{
	k_sem_init(&data_ready, 0, K_SEM_MAX_LIMIT);
	k_timer_init(&crank_timer, crank_isr, NULL);

	/* Pre-header banner via printk: goes out BEFORE we start the
	 * sweep thread / reader, so it can't interleave with event
	 * lines. The CSV header follows immediately. */
	printk("engine_control bench starting "
	       "(target %u samples, %u sweep steps)\n",
	       TOTAL_SAMPLES, (unsigned)SWEEP_STEPS);

	k_thread_priority_set(k_current_get(), 5);

	/* Emit CSV header BEFORE starting the sweep so stdout ordering
	 * is deterministic: header, then events interleaved with sweep
	 * progress printk, then footer. */
	print_csv_header();

	static K_THREAD_STACK_DEFINE(sweep_stack, 1024);
	static struct k_thread sweep_thread;
	k_thread_create(&sweep_thread, sweep_stack,
			K_THREAD_STACK_SIZEOF(sweep_stack),
			(k_thread_entry_t)sweep_driver, NULL, NULL, NULL,
			4, 0, K_NO_WAIT);

	reader_loop();
	k_timer_stop(&crank_timer);
	print_csv_footer();
	return 0;
}
