# STPA Analysis -- ZMS (Zephyr Memory Storage) Subsystem

**Date:** 2026-03-29
**Scope:** Systems-Theoretic Process Analysis for ASIL-D persistent storage safety case
**Subsystem:** `zephyr/subsys/kvss/zms/zms.c` (Zephyr Memory Storage)
**Standard:** ISO 26262 Part 6 (Software Development), ASIL-D

---

## 1. System Description

### 1.1 Purpose

ZMS (Zephyr Memory Storage) provides a persistent key-value store on NOR flash
(or flash-like) devices.  Each entry is identified by a numeric ID (`zms_id_t`,
32-bit or 64-bit) and stores an arbitrary byte blob up to 64 KiB.

### 1.2 Safety-Critical Usage

In an ASIL-D automotive context ZMS is the backing store for:

- **Calibration data** -- sensor offsets, gain tables, look-up curves used by
  control algorithms.  Corruption or loss directly affects actuator commands.
- **Configuration parameters** -- feature flags, communication addresses,
  diagnostic trouble code (DTC) thresholds.
- **Diagnostic counters** -- fault occurrence counters, degradation logs, event
  freeze frames required for OBD-II / UDS compliance.
- **NVM mirrors** -- non-volatile mirrors of RAM variables that must survive
  ECU reset (e.g., odometer, operating hours).

### 1.3 Architecture Overview

```
 Application
     |
     v
 zms_write / zms_read / zms_delete       (API layer, mutex-protected)
     |
     v
 ATE (Allocation Table Entry) management  (log-structured append)
     |            |
     v            v
 Data region    ATE region                 (per-sector layout)
     |
     v
 flash_write / flash_read / flash_erase   (flash driver)
     |
     v
 NOR flash hardware
```

Key design elements:

| Element | Description |
|---------|-------------|
| **Sector** | Fixed-size region of flash.  Minimum 2 sectors; one is always reserved for GC. |
| **ATE** | 16-byte (32-bit ID) or 20-byte (64-bit ID) packed struct containing `crc8`, `cycle_cnt`, `len`, `id`, `offset`/`data`, `data_crc`/`metadata`. |
| **Empty ATE** | Written at sector top after erase; carries `cycle_cnt`, magic, version. |
| **Close ATE** | Written when sector is full; records offset of last ATE. |
| **GC Done ATE** | Marker written after garbage collection completes. |
| **Garbage Collection (GC)** | Relocates live entries from the oldest closed sector to the active sector, then erases the old sector. |
| **Cycle counter** | 8-bit counter per sector, incremented on each erase.  Used to distinguish stale ATEs after flash reuse. |
| **Lookup cache** | Optional hash table mapping ID to most-recent ATE address (compile-time `CONFIG_ZMS_LOOKUP_CACHE`). |
| **Mutex** | `k_mutex zms_lock` serialises all write-path operations. |

### 1.4 Control Structure

```
                  +-----------+
                  | Application|
                  +-----+-----+
                        | zms_write(id, data, len)
                        v
                  +-----+-----+
                  | ZMS API   |  <-- mutex acquire
                  +-----+-----+
                        |
          +-------------+-------------+
          |             |             |
          v             v             v
    +-----+---+   +-----+---+  +-----+-----+
    | ATE Mgmt|   | Data Wrt|  | Sector Mgmt|
    +-----+---+   +----+----+  +-----+-----+
          |             |             |
          +------+------+      +------+------+
                 |             |             |
                 v             v             v
           +-----+---+  +-----+---+  +------+-----+
           |flash_wrt|  |flash_rd |  |flash_erase  |
           +---------+  +---------+  +-------------+
                 |
                 v
           +-----------+
           | NOR Flash |
           +-----------+
```

### 1.5 Controlled Process

The controlled process is the **persistent state on flash**: the set of
(id, value) pairs that the application believes to be durably stored.

---

## 2. Losses

| ID | Loss | Impact |
|----|------|--------|
| **L1** | Loss of safety-critical calibration data | Control algorithm uses default/stale calibration; actuator commands are wrong.  Potential vehicle-level hazard. |
| **L2** | Silent corruption of stored values | Application reads corrupted data and trusts it.  Worse than L1 because no fault is detected. |
| **L3** | Unbounded write latency causing missed real-time deadlines | Safety-critical task blocked waiting for ZMS; watchdog timeout or control loop jitter. |
| **L4** | Flash wear-out causing permanent storage failure | After endurance limit (~100K erases), flash cells fail.  All data permanently lost. |
| **L5** | Inconsistent view of storage across resets | After power loss, application sees a state that never existed (partial old + partial new). |
| **L6** | Denial of write -- safety write rejected when storage is full | Safety-critical counter or configuration update cannot be persisted. |

---

## 3. Hazards

| ID | Hazard | Losses |
|----|--------|--------|
| **H1** | Write overwrites or supersedes valid data without producing a correct new entry | L1, L2 |
| **H2** | GC discards a live entry that is the latest version of a given ID | L1 |
| **H3** | Power loss during write leaves an inconsistent ATE/data pair on flash | L2, L5 |
| **H4** | Power loss during GC loses entries being relocated (source erased, destination incomplete) | L1, L5 |
| **H5** | GC triggered synchronously in safety-critical write path causes unbounded latency | L3 |
| **H6** | All sectors full; safety-critical write returns `-ENOSPC` | L1, L6 |
| **H7** | Flash sector exceeds erase endurance limit | L4 |
| **H8** | Concurrent access without mutex corrupts in-memory pointers (`ate_wra`, `data_wra`) | L1, L2 |
| **H9** | CRC check disabled or bypassed, allowing corrupted ATE to be treated as valid | L2 |
| **H10** | Lookup cache returns stale ATE address, causing GC to skip live entry or read to return old data | L1, L2 |
| **H11** | Cycle counter wraps (8-bit), causing a stale ATE to appear valid | L2 |
| **H12** | `zms_init` recovery logic misidentifies the active sector after power loss | L1, L2, L5 |

---

## 4. Control Actions

The following control actions are analysed:

| CA | Control Action | Description |
|----|----------------|-------------|
| **CA-W** | `zms_write` | Append a new (id, data) entry to flash |
| **CA-R** | `zms_read` / `zms_read_hist` | Read the latest (or historical) entry for a given ID |
| **CA-D** | `zms_delete` | Write a zero-length ATE to logically delete an entry |
| **CA-GC** | `zms_gc` | Relocate live entries from oldest sector, erase it |
| **CA-SC** | `zms_sector_close` | Close the active sector by writing close ATE and garbage ATEs |
| **CA-INIT** | `zms_init` / `zms_mount` | Scan flash to recover write pointers after reset |

---

## 4.1 Unsafe Control Actions (UCAs)

### CA-W: `zms_write`

| UCA ID | Type | Unsafe Control Action | Hazard |
|--------|------|-----------------------|--------|
| **UCA-W1** | Not provided | Write is not executed even though application requested it (returns error spuriously) | H6 |
| **UCA-W2** | Provided incorrectly | Data is written to flash but ATE points to wrong offset (offset calculation error) | H1, H3 |
| **UCA-W3** | Provided incorrectly | ATE `crc8` is computed over wrong bytes (struct packing mismatch) | H9 |
| **UCA-W4** | Provided incorrectly | Data CRC (`data_crc`) not computed when `CONFIG_ZMS_DATA_CRC` enabled but `len <= ZMS_DATA_IN_ATE_SIZE` threshold is wrong | H9 |
| **UCA-W5** | Wrong timing / order | Data blob written to flash but power lost before ATE is written -- orphaned data occupies space but is invisible | H3 |
| **UCA-W6** | Wrong timing / order | ATE written before data blob (code currently writes data first, then ATE -- but reordering by write buffer or flash controller) | H3 |
| **UCA-W7** | Stopped too soon | `zms_flash_al_wrt` partial write: only part of data blob is committed to flash | H1, H2 |
| **UCA-W8** | Provided incorrectly | `NO_DOUBLE_WRITE` comparison reads stale cache entry, concludes data unchanged, skips write | H1 |
| **UCA-W9** | Not provided | Mutex not acquired before `zms_flash_write_entry` (code path exists outside lock in `NO_DOUBLE_WRITE` comparison) | H8 |

### CA-R: `zms_read` / `zms_read_hist`

| UCA ID | Type | Unsafe Control Action | Hazard |
|--------|------|-----------------------|--------|
| **UCA-R1** | Provided incorrectly | Read returns data from a superseded (older) ATE because `zms_find_ate_with_id` walks in wrong direction | H2 |
| **UCA-R2** | Provided incorrectly | Partial read of data blob: `len < wlk_ate.len` succeeds but CRC is not checked (by design -- documented but dangerous) | H9 |
| **UCA-R3** | Provided incorrectly | `data_crc` check disabled at compile time (`CONFIG_ZMS_DATA_CRC` off) -- silent bit-rot on flash goes undetected | H9 |
| **UCA-R4** | Provided incorrectly | Read returns data from lookup cache pointing to an invalidated sector | H10 |
| **UCA-R5** | Not provided | Read returns `-ENOENT` for an entry that exists because the ATE `cycle_cnt` does not match after sector wrap | H11 |

### CA-D: `zms_delete`

| UCA ID | Type | Unsafe Control Action | Hazard |
|--------|------|-----------------------|--------|
| **UCA-D1** | Provided incorrectly | Delete ATE written but previous live ATE for same ID survives in a different sector -- GC later resurfaces the deleted entry | H1 |
| **UCA-D2** | Wrong timing | Delete ATE written just before power loss; on recovery, delete ATE is invalid (incomplete write) and old data reappears | H5 |

### CA-GC: `zms_gc`

| UCA ID | Type | Unsafe Control Action | Hazard |
|--------|------|-----------------------|--------|
| **UCA-GC1** | Provided incorrectly | GC relocates an entry whose ID has a newer version in another sector -- wastes space, no data loss but reduces free space | H6 |
| **UCA-GC2** | Not provided | GC does NOT relocate a live entry because `zms_find_ate_with_id` incorrectly finds a "newer" version (e.g., lookup cache collision) | H2 |
| **UCA-GC3** | Stopped too soon | Power loss after relocating some entries but before `gc_done_ate` is written.  Source sector not yet erased.  On reboot, GC restarts from scratch -- entries already copied are duplicated | H4, H5 |
| **UCA-GC4** | Stopped too soon | Power loss after `gc_done_ate` written but before source sector erased.  Source sector still closed.  On reboot, init erases it -- correct | -- (safe) |
| **UCA-GC5** | Wrong timing | GC runs during a latency-critical write, adding erase + copy latency (potentially hundreds of milliseconds for large sectors on SPI flash) | H5 |
| **UCA-GC6** | Provided incorrectly | `zms_flash_block_move` copies data in `ZMS_BLOCK_SIZE` chunks; interrupted mid-chunk leaves partial data in destination | H4 |
| **UCA-GC7** | Provided incorrectly | After GC, `cycle_cnt` of relocated ATEs set to `previous_cycle` of destination sector.  If destination cycle wraps to match a stale ATE in another sector, validator may accept wrong ATE | H11 |

### CA-SC: `zms_sector_close`

| UCA ID | Type | Unsafe Control Action | Hazard |
|--------|------|-----------------------|--------|
| **UCA-SC1** | Stopped too soon | Power loss after garbage ATEs written but before close ATE -- sector is neither open nor closed.  `zms_init` must handle this ambiguous state | H12 |
| **UCA-SC2** | Provided incorrectly | `close_ate.offset` records wrong position of last ATE -- GC will start from wrong location, skipping or duplicating entries | H2 |
| **UCA-SC3** | Wrong timing | `zms_sector_close` called when `ate_wra < data_wra` -- the garbage ATE loop condition `fs->ate_wra >= fs->data_wra` may underflow | H8 |

### CA-INIT: `zms_init` / `zms_mount`

| UCA ID | Type | Unsafe Control Action | Hazard |
|--------|------|-----------------------|--------|
| **UCA-INIT1** | Provided incorrectly | `zms_recover_last_ate` sets `data_wra` too low -- next write overlaps existing data | H1 |
| **UCA-INIT2** | Provided incorrectly | Active sector misidentified: init picks a closed sector as active, writes collide with valid data | H1, H12 |
| **UCA-INIT3** | Not provided | `zms_mount_force` wipes the partition on any init failure, destroying all stored data | H1 |
| **UCA-INIT4** | Provided incorrectly | GC restart during init (no `gc_done` marker found) erases the active write sector and restarts GC; if the erase fails, data is lost | H4 |
| **UCA-INIT5** | Wrong timing | Init takes unbounded time scanning all sectors on large partitions -- blocks boot beyond watchdog window | H3 |

---

## 5. Causal Scenarios

### 5.1 Software Bugs

| Scenario ID | UCA | Causal Scenario |
|-------------|-----|-----------------|
| **CS-SW1** | UCA-W2 | `SECTOR_OFFSET(fs->data_wra)` cast to `uint32_t` in `entry.offset` is correct only if `data_wra` low 32 bits represent a sector-relative offset.  If the address encoding changes (e.g., 64-bit ID mode changes struct layout), the offset is wrong. |
| **CS-SW2** | UCA-W3 | `zms_ate_crc8_update` skips the first byte (`crc8` field).  If struct packing changes (e.g., `__packed` removed, or compiler inserts padding), CRC covers wrong bytes. |
| **CS-SW3** | UCA-GC2 | Lookup cache hash collision: two IDs map to same cache slot.  `zms_find_ate_with_id` starts from cached address of wrong ID, may terminate early without finding the live ATE. |
| **CS-SW4** | UCA-R1 | `zms_prev_ate` walks backward from `ate_wra`.  If `ate_wra` was incorrectly recovered during init (UCA-INIT1/INIT2), the walk misses the most recent ATE. |
| **CS-SW5** | UCA-SC2 | `close_ate.offset` is set to `SECTOR_OFFSET(fs->ate_wra + fs->ate_size)`.  If `ate_wra` was already decremented past the close position, the offset underflows sector boundary. |
| **CS-SW6** | UCA-W9 | The `NO_DOUBLE_WRITE` code path calls `zms_find_ate_with_id` and `zms_flash_block_cmp` BEFORE acquiring `zms_lock`.  A concurrent writer could modify `ate_wra` between the comparison and the lock acquisition. |
| **CS-SW7** | UCA-GC7 | `cycle_cnt` is 8-bit, wrapping every 256 erases.  `zms_verify_and_increment_cycle_cnt` skips one collision with the close ATE's `cycle_cnt`, but does not handle the case where two other sectors happen to share the same `cycle_cnt`. |

### 5.2 Hardware Faults

| Scenario ID | UCA | Causal Scenario |
|-------------|-----|-----------------|
| **CS-HW1** | UCA-R3 | Flash bit-flip (single-event upset) flips one or more bits in a data blob.  Without `CONFIG_ZMS_DATA_CRC`, the corruption is undetectable. |
| **CS-HW2** | UCA-W7 | NOR flash partial program: flash controller reports success but a page-boundary crossing caused the second page to not be programmed.  `zms_flash_al_wrt` does not read-back to verify. |
| **CS-HW3** | UCA-GC6 | Flash write disturb: programming one cell disturbs an adjacent cell in the same page.  Relocated data in GC destination has a bit error. |
| **CS-HW4** | H7 | Flash endurance exceeded: `flash_erase` succeeds but leaves cells in a metastable state.  Subsequent reads return incorrect data; CRC may or may not catch it depending on which bytes are affected. |
| **CS-HW5** | UCA-SC1 | Brown-out during `flash_write` of close ATE: partial write leaves flash in an intermediate state (some bits programmed, others not).  CRC8 may accidentally pass. |

### 5.3 Concurrency

| Scenario ID | UCA | Causal Scenario |
|-------------|-----|-----------------|
| **CS-CC1** | UCA-W9 | ISR-context call to `zms_read` while thread-context `zms_write` holds the mutex.  `zms_read` does NOT acquire the mutex -- it reads `ate_wra` and `data_wra` non-atomically while the writer is updating them. |
| **CS-CC2** | UCA-W9 | Two threads call `zms_write` simultaneously.  The `NO_DOUBLE_WRITE` comparison is outside the mutex; both threads conclude data has changed and both proceed to write, potentially with `ate_wra` desynchronised. |
| **CS-CC3** | H8 | `zms_calc_free_space` does NOT acquire the mutex.  If called concurrently with `zms_write` + GC, it reads inconsistent `ate_wra`/`data_wra` and may return a negative number that is cast to a large positive `ssize_t`. |

### 5.4 Configuration

| Scenario ID | UCA | Causal Scenario |
|-------------|-----|-----------------|
| **CS-CFG1** | H6 | `sector_count = 2` (minimum allowed): only one sector is usable for data.  After GC, all live entries must fit in one sector; if they do not, the system is permanently full. |
| **CS-CFG2** | H5 | `sector_size` very large (e.g., 256 KiB): a single GC cycle copies up to one full sector of live data plus one erase, potentially taking hundreds of milliseconds on SPI flash. |
| **CS-CFG3** | H9 | `CONFIG_ZMS_DATA_CRC` disabled: no runtime detection of data corruption.  The CRC8 on the ATE catches ATE corruption only, not data blob corruption. |
| **CS-CFG4** | H6 | Large entries (approaching `sector_size - 5*ate_size`) leave almost no room for other entries in the same sector.  A single large write can trigger GC. |
| **CS-CFG5** | H7 | No wear-leveling policy: if the application writes one ID at high frequency, the active sector fills quickly and GC erases the oldest sector repeatedly -- always the same physical sector. |

### 5.5 Power-Loss Sequences

| Scenario ID | UCA | Causal Scenario |
|-------------|-----|-----------------|
| **CS-PL1** | UCA-W5 | Power loss after `zms_flash_data_wrt` but before `zms_flash_ate_wrt`: data is written to flash but no ATE references it.  Space is consumed but entry is invisible.  On next mount, `data_wra` recovery may or may not skip past this orphan depending on whether a later valid ATE has a higher offset. |
| **CS-PL2** | UCA-GC3 | Power loss during GC after some entries copied to destination sector but before `gc_done_ate`: on reboot, `zms_init` detects a closed sector following the active sector with no `gc_done` marker.  It erases the active sector and restarts GC.  **Risk**: if the original active sector had new writes that were not yet in the old sector, those writes are lost. |
| **CS-PL3** | UCA-W6 | Flash controller write buffer reorders: ATE is committed to flash before data blob.  Reader sees ATE (valid CRC8) but data is erased-value.  `CONFIG_ZMS_DATA_CRC` catches this IF a full read is performed. |
| **CS-PL4** | UCA-SC1 | Power loss during `zms_sector_close` after some garbage ATEs written but before close ATE: sector is partially written with junk ATEs.  `zms_init` sees a sector that is neither cleanly open nor cleanly closed.  Recovery scans for the last valid ATE -- the junk ATEs are invalid, so recovery should find the correct position.  **Risk**: if one of the junk ATEs accidentally has a valid CRC8 (probability ~1/256 per ATE), recovery accepts it and sets `ate_wra` incorrectly. |
| **CS-PL5** | H12 | Repeated power loss during GC (the "255 cycle" scenario): each reboot erases the target sector and increments `cycle_cnt`.  After 255 cycles, the `cycle_cnt` wraps and `zms_verify_and_increment_cycle_cnt` detects the collision with the close ATE and increments once more.  However, if three sectors are involved, the second increment may collide with a third sector's `cycle_cnt`. |

---

## 6. Safety Requirements

Each safety requirement is derived from one or more causal scenarios and is
keyed for traceability.

### 6.1 Write Path

| Req ID | Requirement | Derived From |
|--------|-------------|--------------|
| **SWREQ-ZMS-W01** | `zms_write` SHALL write the data blob to flash BEFORE writing the ATE.  The ordering SHALL be enforced by issuing separate `flash_write` calls (no coalescing). | CS-PL1, UCA-W5, UCA-W6 |
| **SWREQ-ZMS-W02** | After writing the ATE, `zms_write` SHALL verify the ATE by reading it back and checking CRC8. | CS-HW2, UCA-W3 |
| **SWREQ-ZMS-W03** | `zms_write` SHALL hold `zms_lock` for the ENTIRE duration of the write operation, including the `NO_DOUBLE_WRITE` comparison. | CS-CC2, UCA-W9 |
| **SWREQ-ZMS-W04** | The ATE `offset` field SHALL be computed from `SECTOR_OFFSET(fs->data_wra)` and SHALL be verified to be within `[0, sector_size - 5*ate_size)` before writing. | CS-SW1, UCA-W2 |
| **SWREQ-ZMS-W05** | `CONFIG_ZMS_DATA_CRC` SHALL be enabled for all ASIL-D configurations.  The Gale build system SHALL enforce this via Kconfig assertion. | CS-CFG3, UCA-R3 |
| **SWREQ-ZMS-W06** | After writing data (when `len > ZMS_DATA_IN_ATE_SIZE`), the write path SHALL read back the data and compare to the source buffer. | CS-HW2, CS-HW3, UCA-W7 |

### 6.2 Read Path

| Req ID | Requirement | Derived From |
|--------|-------------|--------------|
| **SWREQ-ZMS-R01** | `zms_read` SHALL verify data CRC32 on every full read.  Partial reads SHALL return an explicit "CRC not checked" status or be forbidden for ASIL-D IDs. | UCA-R2, CS-HW1 |
| **SWREQ-ZMS-R02** | `zms_read` SHALL validate that the ATE `offset + len` does not exceed the sector boundary before issuing `flash_read`. | UCA-R1, CS-SW4 |
| **SWREQ-ZMS-R03** | The lookup cache SHALL be invalidated atomically with sector erase.  `zms_lookup_cache_invalidate` SHALL be called BEFORE `flash_erase` returns, not after. | UCA-R4, CS-SW3 |

### 6.3 Delete Path

| Req ID | Requirement | Derived From |
|--------|-------------|--------------|
| **SWREQ-ZMS-D01** | A delete ATE (len=0) SHALL be treated as authoritative: GC SHALL NOT relocate any entry whose most-recent ATE is a delete ATE, regardless of which sector the delete ATE resides in. | UCA-D1 |
| **SWREQ-ZMS-D02** | Delete ATEs SHALL be subject to the same CRC8 and `cycle_cnt` validation as data ATEs. | UCA-D2 |

### 6.4 Garbage Collection

| Req ID | Requirement | Derived From |
|--------|-------------|--------------|
| **SWREQ-ZMS-GC01** | GC SHALL NOT erase the source sector until the `gc_done_ate` is durably written to the destination sector. | UCA-GC3, CS-PL2 |
| **SWREQ-ZMS-GC02** | GC SHALL verify each relocated entry by reading back the ATE from the destination sector and comparing CRC8. | UCA-GC6, CS-HW3 |
| **SWREQ-ZMS-GC03** | GC SHALL be idempotent: restarting GC after power loss SHALL produce the same result as completing GC in one pass.  Specifically, duplicated entries SHALL be resolved by the standard "most-recent ATE wins" rule. | UCA-GC3, CS-PL2 |
| **SWREQ-ZMS-GC04** | The worst-case GC latency SHALL be bounded and documented.  For ASIL-D configurations, `sector_size` SHALL be limited such that GC latency does not exceed the system's real-time deadline. | UCA-GC5, CS-CFG2 |
| **SWREQ-ZMS-GC05** | The `cycle_cnt` assignment for relocated ATEs SHALL be verified to not collide with any other sector's `cycle_cnt` in the filesystem, not just the close ATE of the destination sector. | UCA-GC7, CS-SW7 |

### 6.5 Sector Management

| Req ID | Requirement | Derived From |
|--------|-------------|--------------|
| **SWREQ-ZMS-SC01** | `zms_sector_close` SHALL write the close ATE as the LAST operation.  All garbage ATEs SHALL be written first.  A power loss before the close ATE leaves the sector in an open state that `zms_init` can recover. | UCA-SC1, CS-PL4 |
| **SWREQ-ZMS-SC02** | The garbage ATE fill pattern SHALL be chosen such that it CANNOT accidentally produce a valid ATE CRC8 for any `cycle_cnt` value.  (Currently uses `erase_value` -- for `0xFF` erase value, the all-0xFF pattern has CRC8=0xFF, and `cycle_cnt=0xFF` is valid.) | CS-PL4 |
| **SWREQ-ZMS-SC03** | Wear-leveling SHALL be enforced: the GC sector selection SHALL prefer the sector with the lowest erase count (tracked via `cycle_cnt`). | CS-CFG5, H7 |

### 6.6 Initialization and Recovery

| Req ID | Requirement | Derived From |
|--------|-------------|--------------|
| **SWREQ-ZMS-INIT01** | `zms_init` SHALL correctly identify the active sector in all combinations of power-loss states (open/closed/partially-closed sectors). | UCA-INIT2, CS-PL4, CS-PL5 |
| **SWREQ-ZMS-INIT02** | `zms_recover_last_ate` SHALL set `data_wra` to the byte AFTER the highest valid data region referenced by any valid ATE in the active sector. | UCA-INIT1, CS-SW4 |
| **SWREQ-ZMS-INIT03** | `zms_mount` SHALL NOT wipe the partition on failure.  `zms_mount_force` SHALL log a diagnostic event before wiping. | UCA-INIT3 |
| **SWREQ-ZMS-INIT04** | Init-time full-partition scan SHALL complete within a bounded time.  The bound SHALL be configurable and documented (e.g., max sectors * max ATEs per sector * flash read latency). | UCA-INIT5, CS-CFG2 |

### 6.7 Concurrency

| Req ID | Requirement | Derived From |
|--------|-------------|--------------|
| **SWREQ-ZMS-CC01** | All ZMS API functions that access `ate_wra`, `data_wra`, or flash SHALL acquire `zms_lock` before accessing shared state.  `zms_read` and `zms_calc_free_space` are NOT exempt. | CS-CC1, CS-CC3, UCA-W9 |
| **SWREQ-ZMS-CC02** | ZMS API functions SHALL NOT be callable from ISR context.  A runtime check (`__ASSERT_NO_MSG(!k_is_in_isr())`) SHALL be added to every public entry point. | CS-CC1 |

### 6.8 Flash Integrity

| Req ID | Requirement | Derived From |
|--------|-------------|--------------|
| **SWREQ-ZMS-FL01** | After `flash_erase`, the erased sector SHALL be verified by reading back and comparing to `erase_value`.  (Already implemented in `zms_flash_erase_sector`.) | CS-HW4 |
| **SWREQ-ZMS-FL02** | After `flash_write`, the written data SHALL be verified by reading back and comparing to the source buffer. | CS-HW2, CS-HW3 |
| **SWREQ-ZMS-FL03** | If `flash_write` or `flash_erase` returns an error, ZMS SHALL NOT update in-memory pointers (`ate_wra`, `data_wra`).  The operation SHALL be failed atomically. | UCA-W7, CS-HW2 |

---

## 7. Gaps and Mitigations

### GAP-ZMS-1: GC latency in write path

**Risk:** GC is triggered synchronously inside `zms_write` when the active
sector is full.  A single `zms_write` call may trigger `zms_sector_close` +
`zms_gc`, which includes a full sector erase (~50-500ms on NOR flash) plus
copying all live entries.

**Impact:** H5 (L3) -- missed real-time deadlines.

**Current code:** `zms_write` loops up to `sector_count` times calling
`zms_sector_close` + `zms_gc` (lines 1673-1708).

**Mitigation:**
1. Pre-emptive GC: expose `zms_sector_use_next` and call it from a low-priority
   background task when `zms_active_sector_free_space` drops below a threshold.
2. ASIL-D configuration SHALL limit `sector_size` to bound worst-case GC time.
3. Gale wrapper SHALL document the WCET of `zms_write` including GC for each
   supported flash type.

**Status:** Open

---

### GAP-ZMS-2: No reserved capacity for safety-critical writes

**Risk:** The flash partition is shared among all IDs.  Non-safety application
code can fill the partition, causing `-ENOSPC` for a subsequent safety-critical
write.

**Impact:** H6 (L1, L6).

**Current code:** `zms_write` returns `-ENOSPC` when no space remains after
`sector_count` GC cycles (line 1678).

**Mitigation:**
1. Implement a reserved-space mechanism: Gale SHALL maintain a count of bytes
   reserved for ASIL-D IDs.  Non-safety writes SHALL be rejected when free
   space minus reserved space is insufficient.
2. Alternatively, use separate ZMS partitions for safety and non-safety data.

**Status:** Open -- requires ASIL-D enhancement

---

### GAP-ZMS-3: No write-back verification

**Risk:** `zms_flash_al_wrt` and `zms_flash_data_wrt` call `flash_write` but
do not read back the written data to verify it was programmed correctly.

**Impact:** H1 (L1, L2) -- silent write failure.

**Current code:** `zms_flash_al_wrt` (lines 195-232) writes and returns the
flash driver return code, but does not verify contents.

**Mitigation:**
1. Add read-back verification after every `flash_write` in the ZMS write path.
2. On verification failure, return `-EIO` and do NOT advance `ate_wra`/`data_wra`.

**Status:** Open -- SWREQ-ZMS-FL02

---

### GAP-ZMS-4: No redundant storage (shadow blocks)

**Risk:** A single ATE + data blob is the sole record of a key-value pair.  If
the flash page containing the ATE develops a bit error (read disturb, charge
leakage), the entry is lost.

**Impact:** L1, L2.

**Current code:** ZMS stores one ATE per write.  Historical entries survive
until GC, but are not treated as redundant copies.

**Mitigation:**
1. For ASIL-D IDs, write a shadow ATE + data in a second sector.
2. On read, cross-check both copies; if they disagree, use the one with valid
   CRC and log a diagnostic.
3. This is a significant architectural change.  Short-term mitigation: enable
   `CONFIG_ZMS_DATA_CRC` and implement periodic scrubbing (background read +
   CRC check of all entries).

**Status:** Open -- long-term architectural enhancement

---

### GAP-ZMS-5: GC restart vs. resume on power loss

**Risk:** When `zms_init` detects an incomplete GC (closed sector after active
sector, no `gc_done` marker), it erases the active sector and restarts GC from
scratch.  If new writes were added to the active sector before the power loss,
those writes are lost.

**Impact:** H4 (L1, L5).

**Current code:** Lines 1427-1452 in `zms_init`: "No GC Done marker found:
restarting gc" -- erases `fs->ate_wra` sector and calls `zms_gc`.

**Mitigation:**
1. Before erasing the active sector, scan it for valid ATEs with IDs not present
   in the closed source sector.  Relocate those entries to a third sector before
   erasing.
2. Requires `sector_count >= 3` for ASIL-D configurations.
3. Document this as a known limitation for `sector_count == 2`.

**Status:** Open -- critical for ASIL-D

---

### GAP-ZMS-6: Lookup cache hash collision

**Risk:** The lookup cache is a fixed-size hash table without chaining.  Two
IDs that hash to the same slot cause one to evict the other.  During GC, the
evicted ID's cached address may be stale, causing `zms_find_ate_with_id` to
start from the wrong position.

**Impact:** H2, H10 (L1, L2).

**Current code:** `zms_lookup_cache_pos` uses a hash-prospector hash;
`zms_gc` uses the cache as a starting hint for `zms_find_ate_with_id`
(lines 1071-1079).

**Mitigation:**
1. GC SHALL NOT trust the lookup cache as authoritative.  When the cache yields
   a miss (`ZMS_LOOKUP_CACHE_NO_ADDR`), GC already falls back to `fs->ate_wra`
   (line 1075-1078) -- this is correct.
2. Verify: the fallback path walks ALL ATEs, so no entry can be missed.  The
   cache only provides a performance optimisation.
3. Formal verification: model the cache as a non-deterministic oracle and prove
   that GC correctness does not depend on cache accuracy.

**Status:** Partially mitigated (code has fallback), needs formal verification

---

### GAP-ZMS-7: `cycle_cnt` wrap-around (8-bit)

**Risk:** The 8-bit `cycle_cnt` wraps every 256 erases.  After wrap, a stale
ATE from 256 erase cycles ago may have a matching `cycle_cnt` and valid CRC8,
making it appear valid.

**Impact:** H11 (L2).

**Current code:** `zms_sector_close` writes garbage ATEs to overwrite stale ATEs
(lines 742-760).  `zms_verify_and_increment_cycle_cnt` skips one collision
(lines 811-834).

**Mitigation:**
1. The garbage ATE fill in `zms_sector_close` prevents the wrap-around problem
   for most cases.  Verify that the garbage fill covers ALL ATE slots, not just
   those between `ate_wra` and `data_wra`.
2. Edge case: if the sector was never fully filled (only a few ATEs written) and
   then closed, the unfilled ATE slots are still erased-value.  After 256 cycles,
   an erased-value ATE with `cycle_cnt` matching the new cycle is checked: CRC8
   of all-0xFF data is 0xFF.  If `cycle_cnt` happens to be 0xFF, this ATE appears
   valid.  The garbage fill loop addresses this, but only if it runs.
3. For ASIL-D, add a sector-level CRC32 covering all ATEs, verified during GC.

**Status:** Partially mitigated (garbage fill), needs edge-case verification

---

### GAP-ZMS-8: `zms_read` does not hold mutex

**Risk:** `zms_read` and `zms_read_hist` do not acquire `zms_lock`.  A
concurrent `zms_write` + GC can erase the sector being read, causing flash
read errors or returning erased-value data.

**Impact:** H8 (L2).

**Current code:** `zms_read_hist` (line 1720+) does not call `k_mutex_lock`.
Only `zms_write`, `zms_clear`, and `zms_sector_use_next` acquire the mutex.

**Mitigation:**
1. Add `k_mutex_lock(&fs->zms_lock, K_FOREVER)` to `zms_read_hist`.
2. This increases read latency due to mutex contention.
3. Alternative: use a read-write lock (multiple readers, single writer).

**Status:** Open -- SWREQ-ZMS-CC01

---

### GAP-ZMS-9: No ISR guard

**Risk:** ZMS API functions use `k_mutex_lock` which cannot be called from ISR
context.  If an ISR calls `zms_read`, the kernel faults or deadlocks.

**Impact:** System crash.

**Current code:** No `k_is_in_isr()` checks in any ZMS function.

**Mitigation:**
1. Add `__ASSERT_NO_MSG(!k_is_in_isr())` to all public ZMS functions.
2. Document that ZMS is a thread-context-only API.

**Status:** Open -- SWREQ-ZMS-CC02

---

### GAP-ZMS-10: `NO_DOUBLE_WRITE` comparison outside mutex

**Risk:** When `CONFIG_ZMS_NO_DOUBLE_WRITE` is enabled, `zms_write` performs
a flash read + comparison (lines 1592-1658) BEFORE acquiring `zms_lock`
(line 1670).  A concurrent writer can modify the entry between the comparison
and the subsequent write.

**Impact:** H8, H1 -- TOCTOU vulnerability.

**Current code:** The comparison reads the latest ATE and compares data.  If
data matches, returns 0 (no write needed).  The comparison and the subsequent
write are not atomic.

**Mitigation:**
1. Move the `NO_DOUBLE_WRITE` comparison inside the mutex-protected region.
2. This increases mutex hold time but eliminates the race.

**Status:** Open -- SWREQ-ZMS-W03

---

### GAP-ZMS-11: `zms_calc_free_space` is not thread-safe

**Risk:** `zms_calc_free_space` reads `fs->ate_wra`, `fs->data_wra`,
`fs->sector_cycle`, and walks the entire ATE chain without holding `zms_lock`.

**Impact:** H8 -- returns incorrect free space count, potentially a negative
value.

**Current code:** Lines 1879-1983 -- no mutex.

**Mitigation:**
1. Acquire `zms_lock` for the duration of `zms_calc_free_space`.
2. Accept the latency cost, or provide a cached approximation for
   latency-sensitive callers.

**Status:** Open -- SWREQ-ZMS-CC01

---

### GAP-ZMS-12: Erase verification timing

**Risk:** `zms_flash_erase_sector` verifies the erase by reading back the
entire sector and comparing to `erase_value` (lines 397-402).  This is correct
but adds significant latency to the GC path.  On some flash parts, the read-back
may pass immediately after erase but the cells degrade over time (early
retention failure after marginal erase).

**Impact:** H4 (L4) -- flash reliability.

**Mitigation:**
1. For ASIL-D, consider a delayed re-verification (scrub after N milliseconds).
2. Monitor `flash_erase` return codes for marginal erase indicators (if flash
   driver supports them).

**Status:** Accepted risk with monitoring

---

### GAP-ZMS-13: `zms_mount_force` silently destroys all data

**Risk:** `zms_mount_force` calls `zms_wipe_partition` on any `zms_init`
failure, destroying all stored data without explicit application consent.

**Impact:** L1 -- total data loss.

**Current code:** Lines 1532-1537.

**Mitigation:**
1. ASIL-D configurations SHALL NOT use `zms_mount_force`.  Build-time assertion
   to prevent it.
2. If recovery is needed, provide a `zms_mount_recover` that attempts surgical
   repair (erase only corrupted sectors).

**Status:** Open -- SWREQ-ZMS-INIT03

---

### GAP-ZMS-14: ATE struct packing assumption

**Risk:** `struct zms_ate` is `__packed`.  The CRC8 computation assumes the
struct layout matches the flash layout byte-for-byte.  If a different compiler
or a different target interprets `__packed` differently, the CRC is computed
over wrong bytes.

**Impact:** H9 (L2) -- all ATEs appear invalid or, worse, invalid ATEs pass CRC.

**Mitigation:**
1. Add `BUILD_ASSERT(sizeof(struct zms_ate) == 16)` (for 32-bit ID format) or
   `BUILD_ASSERT(sizeof(struct zms_ate) == 16)` (for 64-bit ID format with
   appropriate size).
2. Add `BUILD_ASSERT(offsetof(struct zms_ate, crc8) == 0)`.
3. Gale SHALL provide `_Static_assert` checks for all ATE field offsets.

**Status:** Open -- analogous to Gale GAP-1 (FFI layout)

---

## 8. Verification Matrix

| Safety Requirement | Verification Method | Tool |
|--------------------|---------------------|------|
| SWREQ-ZMS-W01 | Code inspection + power-loss test (Renode) | Manual + Renode |
| SWREQ-ZMS-W02 | Unit test: inject flash-write corruption, verify detection | Ztest |
| SWREQ-ZMS-W03 | Concurrency stress test with TSan | Miri + TSan |
| SWREQ-ZMS-W04 | Kani BMC: prove offset within bounds | Kani |
| SWREQ-ZMS-W05 | Kconfig assertion in Gale overlay | Build system |
| SWREQ-ZMS-W06 | Unit test: inject bit-flip after write, verify detection | Ztest |
| SWREQ-ZMS-R01 | Unit test: corrupt data blob, verify `-EIO` return | Ztest |
| SWREQ-ZMS-R02 | Kani BMC: prove offset+len <= sector_size | Kani |
| SWREQ-ZMS-R03 | Code inspection + race condition test | Manual + TSan |
| SWREQ-ZMS-D01 | Differential test: write-delete-GC-read sequence | Ztest |
| SWREQ-ZMS-D02 | Code inspection (ATE validation is shared) | Manual |
| SWREQ-ZMS-GC01 | Power-loss injection test (Renode) | Renode |
| SWREQ-ZMS-GC02 | Unit test: corrupt relocated ATE, verify detection | Ztest |
| SWREQ-ZMS-GC03 | Power-loss injection test: interrupt GC at each step, verify recovery | Renode |
| SWREQ-ZMS-GC04 | Timing measurement on target hardware | Oscilloscope + Ztest |
| SWREQ-ZMS-GC05 | Verus: model cycle_cnt arithmetic, prove no collision over full range | Verus |
| SWREQ-ZMS-SC01 | Code inspection + power-loss test | Manual + Renode |
| SWREQ-ZMS-SC02 | Verus: prove garbage ATE pattern cannot produce valid CRC8 for any cycle_cnt | Verus |
| SWREQ-ZMS-INIT01 | Exhaustive state-space test: enumerate all sector-state combinations | Kani / Renode |
| SWREQ-ZMS-INIT02 | Unit test: create sectors with known ATE layout, verify data_wra | Ztest |
| SWREQ-ZMS-INIT03 | Code inspection + build-time assertion | Manual + Build |
| SWREQ-ZMS-INIT04 | Timing measurement on max-size partition | Ztest + timer |
| SWREQ-ZMS-CC01 | TSan: concurrent read/write/GC stress test | TSan |
| SWREQ-ZMS-CC02 | Unit test: call from ISR context, verify assertion fires | Ztest |
| SWREQ-ZMS-FL01 | Already implemented; verify with flash error injection | Ztest |
| SWREQ-ZMS-FL02 | Implement + unit test with injected bit-flip | Ztest |
| SWREQ-ZMS-FL03 | Code inspection: verify no pointer advance on error paths | Manual |

---

## 9. Control Structure Diagram -- Power-Loss State Machine

The following state machine captures the sector states that `zms_init` must
handle after an arbitrary power loss:

```
                     +----------+
                     |  ERASED  |  (all 0xFF / erase_value)
                     +----+-----+
                          |
                  add_empty_ate()
                          |
                          v
                     +----+-----+
                     |   OPEN   |  (valid empty ATE, no close ATE)
                     +----+-----+
                          |
                    write ATEs + data
                          |
                          v
                  +-------+--------+
                  | OPEN + DATA    |  (valid empty ATE, ATEs written, no close ATE)
                  +-------+--------+
                          |
               zms_sector_close()
                   |               \
          (completed)        (power loss mid-close)
                   |                      \
                   v                       v
            +------+------+      +--------+---------+
            |   CLOSED    |      | PARTIALLY CLOSED |
            +------+------+      +------------------+
                   |              (some garbage ATEs, no valid close ATE)
                   |              Recovery: treat as OPEN, find last valid ATE
                   |
              zms_gc() target
                   |
         +---------+---------+
         |                   |
    (GC completes)    (power loss during GC)
         |                   |
         v                   v
   +-----+------+    +------+--------+
   | GC DONE    |    | GC INCOMPLETE |
   | (marker)   |    | (no marker)   |
   +-----+------+    +---------------+
         |           Recovery: erase active sector, restart GC
    erase source
         |
         v
   +-----+------+
   |   ERASED   |
   +------------+
```

---

## 10. Summary of Open Items

| Priority | Gap | Description | Effort |
|----------|-----|-------------|--------|
| **P1 (Critical)** | GAP-ZMS-5 | GC restart on power loss can lose new writes | High |
| **P1 (Critical)** | GAP-ZMS-8 | `zms_read` does not hold mutex | Low |
| **P1 (Critical)** | GAP-ZMS-10 | `NO_DOUBLE_WRITE` TOCTOU race | Low |
| **P2 (High)** | GAP-ZMS-2 | No reserved capacity for safety writes | Medium |
| **P2 (High)** | GAP-ZMS-3 | No write-back verification | Medium |
| **P2 (High)** | GAP-ZMS-13 | `zms_mount_force` destroys all data | Low |
| **P2 (High)** | GAP-ZMS-14 | ATE struct packing assumptions | Low |
| **P3 (Medium)** | GAP-ZMS-1 | GC latency in write path | Medium |
| **P3 (Medium)** | GAP-ZMS-7 | 8-bit cycle_cnt wrap-around | Medium |
| **P3 (Medium)** | GAP-ZMS-9 | No ISR guard | Low |
| **P3 (Medium)** | GAP-ZMS-11 | `zms_calc_free_space` not thread-safe | Low |
| **P4 (Low)** | GAP-ZMS-4 | No redundant storage | High |
| **P4 (Low)** | GAP-ZMS-6 | Lookup cache hash collision | Low (verified safe with fallback) |
| **P4 (Low)** | GAP-ZMS-12 | Erase verification timing | Low |

---

## Appendix A: ZMS ATE Structure (32-bit ID format)

```
Offset  Size  Field       Description
------  ----  ----------  -----------
0x00    1     crc8        CRC-8/CCITT of bytes [1..15]
0x01    1     cycle_cnt   Sector erase cycle counter (mod 256)
0x02    2     len         Data length (0 = delete, 0xFFFF = empty ATE header)
0x04    4     id          Entry ID (0xFFFFFFFF = ZMS_HEAD_ID)
0x08    8     data/       Union: inline data (len <= 8) or {offset, data_crc/metadata}
              offset+crc
------  ----
Total: 16 bytes (__packed)
```

## Appendix B: Relationship to Existing Gale STPA

This analysis extends the kernel-primitive STPA (`stpa-analysis.md`) to cover
persistent storage.  The gap numbering is independent (GAP-ZMS-* vs GAP-*) to
avoid confusion.  Cross-references:

- **GAP-1 (FFI layout)** is analogous to **GAP-ZMS-14** (ATE struct packing).
- **GAP-8 (ISR blocking)** is analogous to **GAP-ZMS-9** (no ISR guard).
- **GAP-3 (SMP mutex)** informs **GAP-ZMS-8** (read-path mutex).
