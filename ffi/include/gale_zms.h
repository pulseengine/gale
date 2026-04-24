/*
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale ZMS — verified decision functions for Zephyr Memory Storage.
 *
 * These functions replace the sector state-machine and write-path
 * decisions in subsys/kvss/zms/zms.c using the Extract -> Decide ->
 * Apply pattern. All flash I/O, ATE CRC computation, and data movement
 * remain in C.
 *
 * Verified (Verus + Kani):
 *   gale_zms_write_decide            — ZMS3 (SWREQ-ZMS-P05, O(1) write)
 *   gale_zms_has_space               — ZMS2 (free space consistent)
 *   gale_zms_gc_done_check           — ZMS6 (SWREQ-ZMS-P08, recovery)
 *   gale_zms_sector_close_decide     — sector close validity
 *   gale_zms_pre_gc_scan_decide      — ZMS7 (GAP-ZMS-5 closed)
 *   gale_zms_read_decide             — ZMS8 (GAP-ZMS-8 closed)
 *   gale_zms_no_double_write_decide  — ZMS9 (GAP-ZMS-10 closed)
 */

#ifndef GALE_ZMS_H_
#define GALE_ZMS_H_

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ---- Write decision (ASIL-D O(1) property) ---- */

struct gale_zms_write_decision {
    uint8_t action;   /* 0 = WRITE_OK, 1 = NEEDS_GC */
    uint8_t needs_gc; /* 0 = no GC, 1 = GC required */
};

#define GALE_ZMS_WRITE_OK       0
#define GALE_ZMS_WRITE_NEEDS_GC 1

/**
 * Decide whether a ZMS write can proceed or needs GC.
 *
 * Verified: ZMS3 / SWREQ-ZMS-P05 — when free_space >= data_len + ate_size,
 * the write proceeds without GC (O(1) flash ops).
 *
 * @param sector_count    Number of sectors in the partition (>= 2).
 * @param current_sector  Index of the active sector.
 * @param ate_size        Size of one ATE in bytes.
 * @param free_space      Free space in the active sector.
 * @param data_len        Size of the data payload to write.
 * @return                Decision struct.
 */
struct gale_zms_write_decision gale_zms_write_decide(
    uint32_t sector_count,
    uint32_t current_sector,
    uint32_t ate_size,
    uint32_t free_space,
    uint32_t data_len);

/**
 * Return 1 if `needed` bytes fit in the current sector without GC, 0 otherwise.
 */
uint32_t gale_zms_has_space(
    uint32_t sector_count,
    uint32_t current_sector,
    uint32_t ate_size,
    uint32_t free_space,
    uint32_t needed);

/* ---- Power-loss recovery decision ---- */

struct gale_zms_recovery_decision {
    uint8_t action; /* 0 = NORMAL, 1 = ERASE_AND_RESTART, 2 = RESUME_GC */
};

#define GALE_ZMS_RECOVERY_NORMAL            0
#define GALE_ZMS_RECOVERY_ERASE_AND_RESTART 1
#define GALE_ZMS_RECOVERY_RESUME_GC         2

/**
 * Decide the power-loss recovery action for ZMS.
 *
 * Verified: ZMS6 / SWREQ-ZMS-P08 — the gc_done marker is the sole
 * determinant of recovery action.
 */
struct gale_zms_recovery_decision gale_zms_gc_done_check(
    uint32_t next_sector_closed,
    uint32_t has_gc_done_marker);

/* ---- Sector close decision ---- */

struct gale_zms_close_decision {
    uint8_t action; /* 0 = CAN_CLOSE, 1 = CANNOT_CLOSE */
};

#define GALE_ZMS_CAN_CLOSE    0
#define GALE_ZMS_CANNOT_CLOSE 1

/**
 * Decide whether the sector may be closed.
 *
 * Closes only when ate_wra is at a valid non-zero offset within the
 * sector and ate_wra >= data_wra (ZMS1 invariant).
 */
struct gale_zms_close_decision gale_zms_sector_close_decide(
    uint32_t ate_wra,
    uint32_t data_wra,
    uint32_t sector_size,
    uint8_t  cycle_cnt,
    uint32_t ate_size);

/* ---- Pre-GC scan decision (GAP-ZMS-5 closure) ---- */

struct gale_zms_pre_gc_scan_decision {
    uint8_t  action;         /* see constants below */
    uint32_t relocate_count; /* number of ATEs to relocate before erase */
};

#define GALE_ZMS_PRE_GC_ERASE_ACTIVE        0
#define GALE_ZMS_PRE_GC_RELOCATE_THEN_ERASE 1
#define GALE_ZMS_PRE_GC_INSUFFICIENT_SPARE  2

/**
 * Decide whether the active sector can be safely erased during GC-restart
 * recovery. Closes GAP-ZMS-5 (ZMS7).
 *
 * The caller must first scan the active sector for ATEs whose IDs are
 * absent from the closed source sector (`new_ate_count`). If any such
 * ATEs exist, they must be relocated to a spare sector before erase.
 */
struct gale_zms_pre_gc_scan_decision gale_zms_pre_gc_scan_decide(
    uint32_t sector_count,
    uint32_t new_ate_count,
    uint32_t spare_sectors);

/* ---- Read decision (GAP-ZMS-8 closure) ---- */

struct gale_zms_read_decision {
    uint8_t action; /* 0 = PROCEED, 1 = REJECT */
    int32_t ret;    /* 0 on PROCEED, EPERM on REJECT */
};

#define GALE_ZMS_READ_PROCEED 0
#define GALE_ZMS_READ_REJECT  1

/**
 * Authorize a ZMS read. Closes GAP-ZMS-8 (ZMS8).
 *
 * The caller MUST acquire zms_lock before calling this function and
 * pass mutex_held=1. The Verus model refuses the read otherwise.
 */
struct gale_zms_read_decision gale_zms_read_decide(uint32_t mutex_held);

/* ---- NO_DOUBLE_WRITE decision (GAP-ZMS-10 closure) ---- */

struct gale_zms_no_double_write_decision {
    uint8_t action; /* 0 = PROCEED, 1 = SKIP, 2 = REJECT_TOCTOU */
    int32_t ret;    /* 0 on PROCEED/SKIP, EPERM on REJECT */
};

#define GALE_ZMS_NDW_PROCEED_WRITE  0
#define GALE_ZMS_NDW_SKIP_IDENTICAL 1
#define GALE_ZMS_NDW_REJECT_TOCTOU  2

/**
 * Decide the outcome of the NO_DOUBLE_WRITE dedup compare. Closes
 * GAP-ZMS-10 (ZMS9).
 *
 * The caller MUST hold zms_lock (mutex_held=1) when comparing the
 * incoming payload to the stored value; the compare must execute inside
 * the critical section to eliminate the TOCTOU window.
 */
struct gale_zms_no_double_write_decision gale_zms_no_double_write_decide(
    uint32_t mutex_held,
    uint32_t data_identical);

#ifdef __cplusplus
}
#endif

#endif /* GALE_ZMS_H_ */
