/*
 * SPDX-License-Identifier: Apache-2.0
 * Copyright (c) 2026 PulseEngine
 *
 * Engine-control interrupt benchmark — main app.
 *
 * A k_timer callback (ISR context) simulates a crank-position sensor.
 * Each firing runs a pure-C engine-control algorithm (table lookups
 * for spark advance + fuel duration), then exercises the Zephyr
 * primitive chain Gale replaces (ring_buf_put + k_sem_give).
 *
 * Two timings are recorded per interrupt:
 *
 *   algo_cycles  = t_algo_end - t_entry    # pure C algorithm
 *   handoff_cycles = t_exit - t_algo_end   # ring_buf + sem_give
 *
 * Both histograms are maintained in ISR context (single-CPU, no lock
 * needed). At end-of-run the main thread dumps both as CSV.
 *
 * Baseline vs Gale comparison is done OUTSIDE the firmware: build
 * twice (once without the overlay, once with `prj-gale.conf`), diff
 * the two CSVs via `compare.py`. Algo histogram should be identical
 * across builds (proves measurement isolation); handoff histogram is
 * the meaningful comparison (primitive overhead).
 */

#include <zephyr/kernel.h>
#include <zephyr/sys/ring_buffer.h>
#include <stdio.h>
#include <string.h>

#include "control.h"

/* ------------------------------------------------------------- knobs */

#define RING_CAPACITY_SAMPLES  256
#define HISTOGRAM_BUCKETS      32
#define TOTAL_SAMPLES          150u  /* QEMU-friendly; bump for hardware */

struct sweep_step {
	uint32_t rpm;
	uint32_t samples;
};

static const struct sweep_step sweep[] = {
	{ 1000, 30 },
	{ 2000, 30 },
	{ 4000, 30 },
	{ 6000, 30 },
	{10000, 30 },
};
#define SWEEP_STEPS (sizeof(sweep) / sizeof(sweep[0]))

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
static volatile uint16_t g_load_pct = 45U;
static volatile int16_t  g_coolant  = 85;
static volatile uint8_t  g_knock    = 0;

static volatile uint32_t g_angle      = 0U;
static volatile uint16_t g_seq        = 0U;
static volatile uint16_t g_drops      = 0U;
static volatile uint32_t g_interrupts = 0U;

/* Per-ISR histograms (log-scale buckets in cycles). */
static uint32_t histo_algo[HISTOGRAM_BUCKETS];
static uint32_t histo_handoff[HISTOGRAM_BUCKETS];

static uint32_t min_algo    = UINT32_MAX, max_algo    = 0;
static uint32_t min_handoff = UINT32_MAX, max_handoff = 0;
static uint64_t sum_algo    = 0,          sum_handoff = 0;

static inline uint32_t bucket_of(uint32_t cycles)
{
	uint32_t b = 0;
	while (cycles > 1U && b < (HISTOGRAM_BUCKETS - 1U)) {
		cycles >>= 1;
		b++;
	}
	return b;
}

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

	struct crank_sample s = {
		.angle_deg         = g_angle,
		.rpm               = in.rpm,
		.spark_advance_deg = spark,
		.fuel_duration_us  = fuel,
		.t_entry           = t_entry,
		.t_algo_end        = t_algo_end,
		.t_exit            = 0U,
		.drops_so_far      = g_drops,
		.seq               = g_seq,
	};

	uint32_t wrote = ring_buf_put(&sample_ring, (uint8_t *)&s, sizeof(s));
	if (wrote != sizeof(s)) {
		g_drops++;
	} else {
		k_sem_give(&data_ready);
	}

	uint32_t t_exit = k_cycle_get_32();

	/* Histograms updated in ISR context. Single-CPU, no concurrent
	 * readers: the reporter only reads AFTER all ISRs stop firing. */
	uint32_t algo    = t_algo_end - t_entry;
	uint32_t handoff = t_exit     - t_algo_end;

	histo_algo[bucket_of(algo)]++;
	histo_handoff[bucket_of(handoff)]++;
	if (algo    < min_algo)    min_algo    = algo;
	if (algo    > max_algo)    max_algo    = algo;
	if (handoff < min_handoff) min_handoff = handoff;
	if (handoff > max_handoff) max_handoff = handoff;
	sum_algo    += algo;
	sum_handoff += handoff;

	g_seq++;
	g_angle = (g_angle + 1U) % 720U;
	g_interrupts++;
}

/* ---------------------------------------------------------- reporter */

/* Reader thread: required purely to make k_sem_give do real work.
 * We don't use the ring-buffer contents for measurement — histograms
 * live in ISR-updated globals. */
static uint32_t count = 0;

static void reader_loop(void)
{
	while (count < TOTAL_SAMPLES) {
		k_sem_take(&data_ready, K_FOREVER);
		struct crank_sample s;
		while (ring_buf_get(&sample_ring, (uint8_t *)&s, sizeof(s))
		       == sizeof(s)) {
			count++;
			if (count >= TOTAL_SAMPLES) break;
		}
	}
}

static void dump_histogram(const char *tag, const uint32_t h[])
{
	for (uint32_t b = 0; b < HISTOGRAM_BUCKETS; b++) {
		if (h[b] == 0) continue;
		uint32_t lo = (b == 0) ? 0 : (1u << b);
		uint32_t hi = (1u << (b + 1U)) - 1U;
		printf("H,%s,%u,%u,%u,%u\n", tag, b, lo, hi, h[b]);
	}
}

static void print_csv_report(void)
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
	printf("samples,%u\n", count);
	printf("drops,%u\n", g_drops);
	printf("algo,min,%u\n",    min_algo);
	printf("algo,max,%u\n",    max_algo);
	printf("algo,mean,%llu\n", (unsigned long long)(sum_algo / count));
	printf("handoff,min,%u\n",    min_handoff);
	printf("handoff,max,%u\n",    max_handoff);
	printf("handoff,mean,%llu\n", (unsigned long long)(sum_handoff / count));
	printf("# histogram rows: tag,bucket,low_cycles,high_cycles,count\n");
	dump_histogram("algo",    histo_algo);
	dump_histogram("handoff", histo_handoff);
	printf("=== END ===\n");
}

/* ------------------------------------------------------------- driver */

static void sweep_driver(void)
{
	for (size_t i = 0; i < SWEEP_STEPS; i++) {
		g_rpm = sweep[i].rpm;
		uint32_t period_us = rpm_to_period_us(g_rpm);
		if (period_us == 0) period_us = 1;
		printk("step %u/%u: rpm=%u period_us=%u\n",
		       (unsigned)(i + 1), (unsigned)SWEEP_STEPS,
		       (unsigned)g_rpm, (unsigned)period_us);
		k_timer_start(&crank_timer,
			      K_USEC(period_us),
			      K_USEC(period_us));

		uint32_t start_ints = g_interrupts;
		uint32_t budget_ms = sweep[i].samples * period_us / 1000U
				     + 2000U;
		uint32_t t0 = k_uptime_get_32();
		while ((g_interrupts - start_ints) < sweep[i].samples) {
			if (count >= TOTAL_SAMPLES) return;
			if ((k_uptime_get_32() - t0) > budget_ms) {
				printk("  step %u budget %ums exceeded; ints=%u\n",
				       (unsigned)(i + 1), budget_ms,
				       (unsigned)(g_interrupts - start_ints));
				break;
			}
			k_msleep(10);
		}
	}
	/* Wait for drain. */
	while (count < TOTAL_SAMPLES) {
		k_msleep(5);
	}
}

/* -------------------------------------------------------------- main */

int main(void)
{
	k_sem_init(&data_ready, 0, K_SEM_MAX_LIMIT);
	k_timer_init(&crank_timer, crank_isr, NULL);

	printk("engine_control bench starting "
	       "(target %u samples, %u sweep steps)\n",
	       TOTAL_SAMPLES, (unsigned)SWEEP_STEPS);

	k_thread_priority_set(k_current_get(), 5);

	static K_THREAD_STACK_DEFINE(sweep_stack, 1024);
	static struct k_thread sweep_thread;
	k_thread_create(&sweep_thread, sweep_stack,
			K_THREAD_STACK_SIZEOF(sweep_stack),
			(k_thread_entry_t)sweep_driver, NULL, NULL, NULL,
			4, 0, K_NO_WAIT);

	reader_loop();
	k_timer_stop(&crank_timer);
	print_csv_report();
	return 0;
}
