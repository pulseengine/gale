/*
 * SPDX-License-Identifier: Apache-2.0
 * Copyright (c) 2026 PulseEngine
 *
 * DWT counter snapshot — implementation.
 *
 * Direct MMIO instead of CMSIS macros to keep this file independent
 * of which CMSIS header bundle Zephyr happens to ship for the target
 * SoC. The register addresses below are ARMv7-M architecture-defined
 * (DDI0403E.e §C1.6, "DWT Memory Map") and identical across Cortex-M3,
 * M4, and M7. ARMv8-M (M33/M55) is also compatible at this level.
 */

#include <stdint.h>
#include <stdio.h>

#include "smart_dwt.h"

#define DCB_DEMCR_ADDR     0xE000EDFCu  /* Debug Exception and Monitor Control */
#define DCB_DEMCR_TRCENA   (1u << 24)

#define DWT_CTRL_ADDR      0xE0001000u
#define DWT_CYCCNT_ADDR    0xE0001004u
#define DWT_CPICNT_ADDR    0xE0001008u
#define DWT_EXCCNT_ADDR    0xE000100Cu
#define DWT_SLEEPCNT_ADDR  0xE0001010u
#define DWT_LSUCNT_ADDR    0xE0001014u
#define DWT_FOLDCNT_ADDR   0xE0001018u

#define DWT_CTRL_CYCCNTENA (1u << 0)
#define DWT_CTRL_CPIEVTENA (1u << 17)
#define DWT_CTRL_EXCEVTENA (1u << 18)
#define DWT_CTRL_SLEEPEVT  (1u << 19)
#define DWT_CTRL_LSUEVTENA (1u << 20)
#define DWT_CTRL_FOLDEVTNA (1u << 21)
/* Hardware-level event-counter enables. The "EVTENA" bits gate the
 * external trace output for those events; the counters themselves
 * increment whenever the corresponding microarchitectural event
 * occurs. We set them anyway so that, if a future bench is hooked
 * up to a SWO trace probe, the events are visible there too. */

#define MMIO32(addr) (*(volatile uint32_t *)(uintptr_t)(addr))

void smart_dwt_enable(void)
{
	MMIO32(DCB_DEMCR_ADDR) |= DCB_DEMCR_TRCENA;

	/* Reset the counters to a known starting point. CYCCNT is a
	 * 32-bit field; the others are 8-bit fields in their own
	 * registers but writing 0 is the conventional reset. */
	MMIO32(DWT_CYCCNT_ADDR)   = 0u;
	MMIO32(DWT_CPICNT_ADDR)   = 0u;
	MMIO32(DWT_EXCCNT_ADDR)   = 0u;
	MMIO32(DWT_SLEEPCNT_ADDR) = 0u;
	MMIO32(DWT_LSUCNT_ADDR)   = 0u;
	MMIO32(DWT_FOLDCNT_ADDR)  = 0u;

	uint32_t ctrl = MMIO32(DWT_CTRL_ADDR);
	ctrl |= DWT_CTRL_CYCCNTENA
	     |  DWT_CTRL_CPIEVTENA
	     |  DWT_CTRL_EXCEVTENA
	     |  DWT_CTRL_SLEEPEVT
	     |  DWT_CTRL_LSUEVTENA
	     |  DWT_CTRL_FOLDEVTNA;
	MMIO32(DWT_CTRL_ADDR) = ctrl;
}

void smart_dwt_snapshot(struct dwt_snapshot *out)
{
	out->cyccnt   = MMIO32(DWT_CYCCNT_ADDR);
	out->cpicnt   = (uint8_t)(MMIO32(DWT_CPICNT_ADDR)   & 0xFFu);
	out->exccnt   = (uint8_t)(MMIO32(DWT_EXCCNT_ADDR)   & 0xFFu);
	out->sleepcnt = (uint8_t)(MMIO32(DWT_SLEEPCNT_ADDR) & 0xFFu);
	out->lsucnt   = (uint8_t)(MMIO32(DWT_LSUCNT_ADDR)   & 0xFFu);
	out->foldcnt  = (uint8_t)(MMIO32(DWT_FOLDCNT_ADDR)  & 0xFFu);
}

void smart_dwt_emit(const char *at, const struct dwt_snapshot *s)
{
	printf("D,%s,%u,%u,%u,%u,%u,%u\n",
	       at,
	       (unsigned)s->cyccnt,
	       (unsigned)s->cpicnt,
	       (unsigned)s->exccnt,
	       (unsigned)s->sleepcnt,
	       (unsigned)s->lsucnt,
	       (unsigned)s->foldcnt);
}
