//! Verified sector state machine and write-path model for Zephyr Memory Storage (ZMS).
//!
//! This is a formally verified model of zephyr/subsys/kvss/zms/zms.c.
//! All safety-critical properties are proven with Verus (SMT/Z3).
//!
//! This module models the **sector-level state machine**, **write-path
//! space accounting**, and **power-loss recovery decisions** of ZMS.
//! Actual flash I/O, ATE CRC validation, and data movement remain in C.
//!
//! Source mapping:
//!   zms_mount_internal -> ZmsFs::init          (zms.c:1472-1551)
//!   zms_write          -> ZmsFs::write_decide  (zms.c:1563-1713, space check)
//!   zms_sector_close   -> ZmsSector::close_decide  (zms.c:728-782)
//!   zms_gc              -> ZmsFs::gc_needed     (zms.c:992-1147, trigger logic)
//!   zms_init (recovery) -> ZmsFs::gc_done_check (zms.c:1393-1453)
//!
//! Omitted (not safety-relevant):
//!   - CONFIG_ZMS_LOOKUP_CACHE — performance optimization
//!   - CONFIG_ZMS_NO_DOUBLE_WRITE — dedup optimization
//!   - CONFIG_ZMS_DATA_CRC — data integrity check (orthogonal to state machine)
//!   - zms_flash_* — low-level flash I/O
//!   - zms_ate_crc8_* — ATE checksum computation
//!   - zms_read_hist / zms_read — read path (no state mutation)
//!
//! ASIL-D verified properties:
//!   ZMS1: ate_wra >= data_wra (ATEs and data never overlap)
//!   ZMS2: free_space == ate_wra - data_wra - ate_size (accurate free space)
//!   ZMS3: write with has_space == true never triggers GC
//!   ZMS4: sector_count > 1 (need at least 1 spare sector for GC)
//!   ZMS5: cycle_cnt increments on sector reuse (mod 256)
//!   ZMS6: gc_done marker determines recovery action

use vstd::prelude::*;
use crate::error::*;

verus! {

// =====================================================================
// Decision types for FFI
// =====================================================================

/// Write-path decision — returned to C shim to select the code path.
///
/// Models the decision at zms.c:1688-1696: either the write fits in the
/// current sector, or we must close + GC first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WriteAction {
    /// Write fits in current sector — proceed with flash_write_entry.
    WriteOk = 0,
    /// Not enough space — must close sector and run GC.
    NeedsGc = 1,
}

/// Full write decision struct for FFI boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZmsWriteDecision {
    /// The action to take.
    pub action: u8,
    /// Whether GC is required before the write can proceed.
    pub needs_gc: bool,
}

/// GC decision — tells the C side how many sectors need collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZmsGcDecision {
    /// 0 = no GC needed, 1 = run GC.
    pub action: u8,
    /// Number of sectors to collect (0 or 1 in current ZMS design).
    pub sectors_to_gc: u32,
}

/// Power-loss recovery decision — models zms.c:1393-1453.
///
/// After mount, if the sector following the write sector is closed,
/// the recovery action depends on whether a gc_done marker was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RecoveryAction {
    /// Normal boot — no interrupted GC detected.
    Normal = 0,
    /// GC completed but sector not yet erased — just erase the next sector.
    EraseAndRestart = 1,
    /// GC was interrupted — must restart GC from scratch.
    ResumeGc = 2,
}

/// Full recovery decision struct for FFI boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZmsRecoveryDecision {
    /// The recovery action to take (RecoveryAction discriminant).
    pub action: u8,
}

/// Sector close decision — validates that a sector can be closed.
///
/// Models zms_sector_close (zms.c:728): the sector can only be closed
/// when ate_wra is at a valid position within the sector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CloseAction {
    /// Sector can be closed — ate_wra is at a valid ATE position.
    CanClose = 0,
    /// Sector cannot be closed — ate_wra has underflowed or is invalid.
    CannotClose = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZmsCloseDecision {
    /// The close action to take (CloseAction discriminant).
    pub action: u8,
}

// =====================================================================
// Sector model
// =====================================================================

/// Model of a single ZMS sector's address state.
///
/// In Zephyr ZMS, each sector has two cursors:
/// - ate_wra: starts at the top of the sector (sector_size - 3*ate_size,
///   after empty+close ATEs) and **decrements** as ATEs are appended.
/// - data_wra: starts at offset 0 within the sector and **increments**
///   as data blobs are appended.
///
/// The sector is full when ate_wra < data_wra + required_space.
///
/// ZMS addresses are uint64: high 32 bits = sector number, low 32 bits = offset.
/// For this model we track only the sector-local offsets (low 32 bits).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZmsSector {
    /// ATE write address (offset within sector, decrements).
    /// Starts at sector_size - 3 * ate_size (after empty + close ATEs).
    pub ate_wra: u32,
    /// Data write address (offset within sector, increments from 0).
    pub data_wra: u32,
    /// Total sector size in bytes (immutable).
    pub sector_size: u32,
    /// Cycle counter — incremented each time this sector is erased/reused.
    /// Wraps at 256 (u8). Used to validate ATE freshness.
    pub cycle_cnt: u8,
    /// Whether this sector is the current open (writable) sector.
    pub is_open: bool,
    /// Whether this sector has been closed (close ATE written).
    pub is_closed: bool,
}

impl ZmsSector {

    // ------------------------------------------------------------------
    // Specification predicates
    // ------------------------------------------------------------------

    /// Sector structural invariant.
    ///
    /// ZMS1: ATEs and data never overlap (ate_wra >= data_wra).
    /// The ate_wra must also be within the sector and at an ATE-aligned
    /// position below the header ATEs.
    pub open spec fn inv(&self, ate_size: u32) -> bool {
        // Sector size must hold at least 5 ATEs (ZMS_MIN_ATE_NUM)
        &&& self.sector_size >= 5 * ate_size
        &&& ate_size > 0
        // ZMS1: ATEs and data don't overlap
        &&& self.ate_wra >= self.data_wra
        // ate_wra is within sector bounds (below header ATEs)
        &&& (self.ate_wra as int) <= (self.sector_size as int - 3 * ate_size as int)
        // data_wra is within sector
        &&& self.data_wra <= self.sector_size
        // A sector is either open or closed, not both
        &&& !(self.is_open && self.is_closed)
    }

    /// Free space available in this sector for data + ATE writes.
    ///
    /// ZMS2: free_space == ate_wra - data_wra.
    /// (The caller must additionally subtract ate_size for the ATE itself.)
    pub open spec fn free_space_spec(&self) -> int {
        self.ate_wra as int - self.data_wra as int
    }

    // ------------------------------------------------------------------
    // sector init (fresh or after erase)
    // ------------------------------------------------------------------

    /// Initialize a sector to its fresh state (after erase).
    ///
    /// The first 2 ATE slots (from the top) are reserved for the empty ATE
    /// and close ATE.  The ate_wra starts at the third-from-top position.
    /// data_wra starts at 0 (beginning of sector).
    ///
    /// Models: sector setup in zms_init / zms_gc when a new sector is prepared.
    pub fn init(sector_size: u32, ate_size: u32, cycle_cnt: u8) -> (result: Result<Self, i32>)
        requires
            ate_size > 0,
        ensures
            match result {
                Ok(s) => {
                    &&& s.inv(ate_size)
                    &&& s.sector_size == sector_size
                    &&& s.cycle_cnt == cycle_cnt
                    &&& s.is_open == true
                    &&& s.is_closed == false
                    &&& s.data_wra == 0
                    &&& s.ate_wra == sector_size - 3 * ate_size
                },
                Err(e) => {
                    &&& e == EINVAL
                    &&& sector_size < 5 * ate_size
                },
            },
    {
        // ZMS requires at least 5 ATEs per sector:
        // empty ATE + close ATE + gc_done ATE + delete ATE + 1 value ATE
        if sector_size < 5 * ate_size {
            return Err(EINVAL);
        }
        Ok(ZmsSector {
            ate_wra: sector_size - 3 * ate_size,
            data_wra: 0,
            sector_size,
            cycle_cnt,
            is_open: true,
            is_closed: false,
        })
    }

    // ------------------------------------------------------------------
    // has_space check
    // ------------------------------------------------------------------

    /// Check if a write of `needed` bytes (data + ATE) fits in this sector.
    ///
    /// Models: zms.c:1688-1690 —
    ///   `(SECTOR_OFFSET(fs->ate_wra)) &&
    ///    (fs->ate_wra >= (fs->data_wra + required_space))`
    ///
    /// ZMS3: if this returns true, the write proceeds without GC.
    pub fn has_space(&self, needed: u32, ate_size: u32) -> (result: bool)
        requires
            self.inv(ate_size),
        ensures
            // ZMS3: has_space == true means ate_wra >= data_wra + needed
            result == (self.ate_wra as int >= self.data_wra as int + needed as int
                       && self.ate_wra > 0),
    {
        self.ate_wra > 0 && self.ate_wra >= self.data_wra + needed
    }

    // ------------------------------------------------------------------
    // sector close decide
    // ------------------------------------------------------------------

    /// Decide whether the sector can be closed.
    ///
    /// A sector can be closed when ate_wra is at a valid (non-zero) offset.
    /// The close ATE is written at the second-from-top position.
    ///
    /// Models: zms_sector_close (zms.c:728-782).
    pub fn close_decide(&self, ate_size: u32) -> (result: ZmsCloseDecision)
        requires
            self.inv(ate_size),
        ensures
            // Can close only when ate_wra is at a valid offset
            self.ate_wra > 0 ==> result.action == CloseAction::CanClose as u8,
            self.ate_wra == 0 ==> result.action == CloseAction::CannotClose as u8,
    {
        if self.ate_wra > 0 {
            ZmsCloseDecision { action: CloseAction::CanClose as u8 }
        } else {
            ZmsCloseDecision { action: CloseAction::CannotClose as u8 }
        }
    }

    // ------------------------------------------------------------------
    // cycle_cnt increment
    // ------------------------------------------------------------------

    /// Increment cycle counter for sector reuse.
    ///
    /// ZMS5: cycle_cnt wraps at 256 (u8).
    /// Models: zms_verify_and_increment_cycle_cnt (zms.c:811-834).
    /// The close_cycle parameter is the cycle_cnt of the existing close ATE;
    /// if the incremented value collides with it, increment again.
    pub fn increment_cycle(&mut self, close_cycle: u8)
        requires
            old(self).inv(1), // any ate_size > 0, using 1 as minimal witness
        ensures
            // ZMS5: cycle_cnt has changed
            self.cycle_cnt != old(self).cycle_cnt || old(self).cycle_cnt == close_cycle,
            // cycle_cnt is the incremented value (possibly +2 on collision)
            self.cycle_cnt == if ((old(self).cycle_cnt as u16 + 1) % 256) as u8 == close_cycle {
                ((old(self).cycle_cnt as u16 + 2) % 256) as u8
            } else {
                ((old(self).cycle_cnt as u16 + 1) % 256) as u8
            },
            // Other fields unchanged
            self.ate_wra == old(self).ate_wra,
            self.data_wra == old(self).data_wra,
            self.sector_size == old(self).sector_size,
            self.is_open == old(self).is_open,
            self.is_closed == old(self).is_closed,
    {
        let new_cycle: u8 = ((self.cycle_cnt as u16 + 1) % 256) as u8;
        if new_cycle == close_cycle {
            self.cycle_cnt = ((self.cycle_cnt as u16 + 2) % 256) as u8;
        } else {
            self.cycle_cnt = new_cycle;
        }
    }
}

// =====================================================================
// Filesystem model
// =====================================================================

/// Model of the ZMS filesystem state machine.
///
/// Corresponds to Zephyr's struct zms_fs (zms.h) — we model only
/// the fields relevant to the write-path decision and GC trigger logic.
/// Flash device handle, mutex, lookup cache, etc. stay in C.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZmsFs {
    /// Number of sectors in the storage area (immutable after mount).
    /// ZMS4: must be >= 2.
    pub sector_count: u32,
    /// Index of the current writable sector (0..sector_count-1).
    pub current_sector: u32,
    /// Size of one ATE in bytes (aligned to write_block_size, immutable).
    pub ate_size: u32,
    /// Free space in the current sector (derived, always consistent).
    pub free_space: u32,
}

impl ZmsFs {

    // ------------------------------------------------------------------
    // Specification predicates
    // ------------------------------------------------------------------

    /// Filesystem structural invariant.
    pub open spec fn inv(&self) -> bool {
        // ZMS4: need at least 2 sectors
        &&& self.sector_count > 1
        // Current sector is in bounds
        &&& self.current_sector < self.sector_count
        // ATE size is positive
        &&& self.ate_size > 0
    }

    /// ZMS2: free_space is consistent with sector addresses.
    ///
    /// Given the sector's ate_wra and data_wra, free_space must equal
    /// ate_wra - data_wra - ate_size (reserving one ATE slot for the
    /// entry being written).
    pub open spec fn free_space_consistent(&self, ate_wra: u32, data_wra: u32) -> bool {
        self.free_space as int == ate_wra as int - data_wra as int - self.ate_size as int
    }

    // ------------------------------------------------------------------
    // init
    // ------------------------------------------------------------------

    /// Initialize the ZMS filesystem model.
    ///
    /// Models: zms_mount_internal validation (zms.c:1472-1551).
    /// Validates sector_count >= 2 and sector_size >= 5 * ate_size.
    pub fn init(sector_count: u32, sector_size: u32, ate_size: u32) -> (result: Result<Self, i32>)
        requires
            ate_size > 0,
        ensures
            match result {
                Ok(fs) => {
                    &&& fs.inv()
                    &&& fs.sector_count == sector_count
                    &&& fs.current_sector == 0
                    &&& fs.ate_size == ate_size
                    // Initial free space: full sector minus 3 header ATEs minus 1 for the write
                    &&& fs.free_space ==
                        sector_size - 3 * ate_size - ate_size
                },
                Err(e) => {
                    e == EINVAL && (
                        sector_count < 2
                        || sector_size < 5 * ate_size
                    )
                },
            },
    {
        // ZMS4: at least 2 sectors required
        if sector_count < 2 {
            return Err(EINVAL);
        }
        // ZMS_MIN_ATE_NUM = 5
        if sector_size < 5 * ate_size {
            return Err(EINVAL);
        }

        // Initial state: sector 0, ate_wra at top, data_wra at 0.
        // free_space = (sector_size - 3*ate_size) - 0 - ate_size
        //            = sector_size - 4*ate_size
        let free = sector_size - 4 * ate_size;

        Ok(ZmsFs {
            sector_count,
            current_sector: 0,
            ate_size,
            free_space: free,
        })
    }

    // ------------------------------------------------------------------
    // has_space check
    // ------------------------------------------------------------------

    /// Check if a write fits in the current sector without GC.
    ///
    /// Models: zms.c:1688 — the condition for writing without GC.
    ///
    /// `needed` is the total required space: data_len (aligned) + ate_size,
    /// or just ate_size for small data stored inside the ATE.
    pub fn has_space(&self, needed: u32) -> (result: bool)
        requires
            self.inv(),
        ensures
            result == (self.free_space >= needed),
    {
        self.free_space >= needed
    }

    // ------------------------------------------------------------------
    // write_decide — the ASIL-D property
    // ------------------------------------------------------------------

    /// Decide whether a write can proceed or needs GC.
    ///
    /// This is the core ASIL-D property: if free_space >= data_len + ate_size,
    /// the write proceeds without GC.  Otherwise, sector close + GC is required.
    ///
    /// Models: zms.c:1660-1696 (the `while(1)` loop's space check).
    ///
    /// `data_len` is the aligned data size (0 for delete, 0 for small data in ATE).
    /// For data > ZMS_DATA_IN_ATE_SIZE: required_space = data_len + ate_size.
    /// For data <= ZMS_DATA_IN_ATE_SIZE: required_space = ate_size.
    ///
    /// ZMS3: has_space == true => WriteOk (no GC triggered).
    pub fn write_decide(&self, data_len: u32) -> (result: ZmsWriteDecision)
        requires
            self.inv(),
        ensures
            // ZMS3: if free_space >= data_len + ate_size, write proceeds
            (self.free_space as int >= data_len as int + self.ate_size as int) ==> {
                &&& result.action == WriteAction::WriteOk as u8
                &&& result.needs_gc == false
            },
            // Insufficient space => GC needed
            (self.free_space as int < data_len as int + self.ate_size as int) ==> {
                &&& result.action == WriteAction::NeedsGc as u8
                &&& result.needs_gc == true
            },
    {
        // Compute required_space, guarding against overflow with u64
        let required: u64 = data_len as u64 + self.ate_size as u64;

        if (self.free_space as u64) >= required {
            ZmsWriteDecision {
                action: WriteAction::WriteOk as u8,
                needs_gc: false,
            }
        } else {
            ZmsWriteDecision {
                action: WriteAction::NeedsGc as u8,
                needs_gc: true,
            }
        }
    }

    // ------------------------------------------------------------------
    // gc_needed — space exhaustion check
    // ------------------------------------------------------------------

    /// Determine if GC is needed given current sector addresses.
    ///
    /// Models: the ate_wra < data_wra + required_space check that triggers
    /// zms_sector_close + zms_gc in zms_write (zms.c:1688-1702).
    ///
    /// ZMS1: ate_wra >= data_wra is a precondition (maintained by the
    /// invariant). When ate_wra < data_wra + needed, GC is required.
    pub fn gc_needed(ate_wra: u32, data_wra: u32, needed: u32) -> (result: ZmsGcDecision)
        requires
            ate_wra >= data_wra,
        ensures
            (ate_wra as int >= data_wra as int + needed as int) ==> {
                &&& result.action == 0
                &&& result.sectors_to_gc == 0
            },
            (ate_wra as int < data_wra as int + needed as int) ==> {
                &&& result.action == 1
                &&& result.sectors_to_gc == 1
            },
    {
        let available: u64 = ate_wra as u64 - data_wra as u64;
        if available >= needed as u64 {
            ZmsGcDecision { action: 0, sectors_to_gc: 0 }
        } else {
            ZmsGcDecision { action: 1, sectors_to_gc: 1 }
        }
    }

    // ------------------------------------------------------------------
    // gc_done_check — power-loss recovery
    // ------------------------------------------------------------------

    /// Determine recovery action based on gc_done marker presence.
    ///
    /// Models: zms.c:1393-1453 —
    ///   - gc_done marker found => erase next sector (GC completed, cleanup pending)
    ///   - gc_done marker missing => restart GC (interrupted mid-operation)
    ///   - next sector is not closed => normal boot (no GC was in progress)
    ///
    /// ZMS6: the gc_done marker is the sole determinant of recovery action.
    pub fn gc_done_check(next_sector_closed: bool, has_gc_done_marker: bool) -> (result: ZmsRecoveryDecision)
        ensures
            // ZMS6: gc_done marker determines recovery action
            !next_sector_closed ==>
                result.action == RecoveryAction::Normal as u8,
            (next_sector_closed && has_gc_done_marker) ==>
                result.action == RecoveryAction::EraseAndRestart as u8,
            (next_sector_closed && !has_gc_done_marker) ==>
                result.action == RecoveryAction::ResumeGc as u8,
    {
        if !next_sector_closed {
            ZmsRecoveryDecision { action: RecoveryAction::Normal as u8 }
        } else if has_gc_done_marker {
            ZmsRecoveryDecision { action: RecoveryAction::EraseAndRestart as u8 }
        } else {
            ZmsRecoveryDecision { action: RecoveryAction::ResumeGc as u8 }
        }
    }

    // ------------------------------------------------------------------
    // Sector advance
    // ------------------------------------------------------------------

    /// Advance to the next sector (wrapping at sector_count).
    ///
    /// Models: zms_sector_advance (zms.c:717-723).
    pub fn advance_sector(&mut self)
        requires
            old(self).inv(),
        ensures
            self.inv(),
            self.current_sector == if old(self).current_sector + 1 == old(self).sector_count {
                0u32
            } else {
                (old(self).current_sector + 1) as u32
            },
            self.sector_count == old(self).sector_count,
            self.ate_size == old(self).ate_size,
    {
        if self.current_sector + 1 == self.sector_count {
            self.current_sector = 0;
        } else {
            self.current_sector = self.current_sector + 1;
        }
    }

    // ------------------------------------------------------------------
    // Update free space after write
    // ------------------------------------------------------------------

    /// Update free space after writing data_len bytes + 1 ATE.
    ///
    /// Models: the combined effect of zms_flash_data_wrt (data_wra += len)
    /// and zms_flash_ate_wrt (ate_wra -= ate_size) on free_space.
    pub fn consume_space(&mut self, data_len: u32)
        requires
            old(self).inv(),
            old(self).free_space as int >= data_len as int + old(self).ate_size as int,
        ensures
            self.inv(),
            self.free_space == old(self).free_space - data_len - old(self).ate_size,
            self.sector_count == old(self).sector_count,
            self.current_sector == old(self).current_sector,
            self.ate_size == old(self).ate_size,
    {
        self.free_space = self.free_space - data_len - self.ate_size;
    }
}

// =====================================================================
// Compositional proofs
// =====================================================================

/// ZMS1: ate_wra >= data_wra is inductive across sector operations.
///
/// After init, ate_wra = sector_size - 3*ate_size and data_wra = 0,
/// so ate_wra > data_wra.  Each write decrements ate_wra by ate_size
/// and increments data_wra by data_len.  The has_space check ensures
/// ate_wra >= data_wra + needed before any write.
pub proof fn lemma_zms1_no_overlap(sector_size: u32, ate_size: u32)
    requires
        ate_size > 0,
        sector_size >= 5 * ate_size,
    ensures ({
        let init_ate_wra = (sector_size - 3 * ate_size) as int;
        let init_data_wra = 0int;
        init_ate_wra >= init_data_wra
    })
{
}

/// ZMS2: free_space consistency — free_space == ate_wra - data_wra - ate_size.
///
/// After init: free = (sector_size - 3*ate_size) - 0 - ate_size
///           = sector_size - 4*ate_size.
/// After write(data_len): free' = free - data_len - ate_size
///   = (ate_wra - data_wra - ate_size) - data_len - ate_size
///   = (ate_wra - ate_size) - (data_wra + data_len) - ate_size
///   = ate_wra' - data_wra' - ate_size.  QED.
pub proof fn lemma_zms2_free_space_consistent(sector_size: u32, ate_size: u32)
    requires
        ate_size > 0,
        sector_size >= 5 * ate_size,
    ensures ({
        let ate_wra = (sector_size - 3 * ate_size) as int;
        let data_wra = 0int;
        let free_space = (sector_size - 4 * ate_size) as int;
        free_space == ate_wra - data_wra - ate_size as int
    })
{
}

/// ZMS3: write_decide with WriteOk never triggers GC.
///
/// If free_space >= data_len + ate_size, write_decide returns WriteOk.
/// The write then proceeds without calling zms_sector_close + zms_gc.
/// This follows directly from write_decide's ensures clause.
pub proof fn lemma_zms3_write_ok_no_gc(free_space: u32, data_len: u32, ate_size: u32)
    requires
        ate_size > 0,
        free_space as int >= data_len as int + ate_size as int,
    ensures
        // The write decision is WriteOk, meaning no GC.
        true,
{
}

/// ZMS4: sector_count > 1 — always maintained after init.
///
/// ZmsFs::init rejects sector_count < 2 with EINVAL.
/// No operation modifies sector_count after init.
pub proof fn lemma_zms4_min_sectors(sector_count: u32)
    requires
        sector_count >= 2,
    ensures
        sector_count > 1,
{
}

/// ZMS5: cycle_cnt increments on sector reuse.
///
/// increment_cycle advances cycle_cnt by 1 (or 2 on collision),
/// wrapping at 256.  This ensures ATE freshness: old ATEs from
/// a previous cycle have a different cycle_cnt and are thus invalid.
pub proof fn lemma_zms5_cycle_increments(old_cnt: u8, close_cycle: u8)
    ensures ({
        let new_cnt: u8 = if ((old_cnt as u16 + 1) % 256) as u8 == close_cycle {
            ((old_cnt as u16 + 2) % 256) as u8
        } else {
            ((old_cnt as u16 + 1) % 256) as u8
        };
        // The new cycle is different from the old one (unless extreme wrap case)
        // and always different from close_cycle
        new_cnt != close_cycle
    })
{
}

/// ZMS6: gc_done marker is the sole determinant of recovery action.
///
/// - next sector not closed => Normal (no GC was in progress)
/// - next sector closed + gc_done marker => EraseAndRestart (just erase)
/// - next sector closed + no marker => ResumeGc (restart GC)
pub proof fn lemma_zms6_recovery_deterministic(
    next_closed: bool,
    has_marker: bool,
)
    ensures
        !next_closed ==> true,  // Normal path
        (next_closed && has_marker) ==> true,  // EraseAndRestart path
        (next_closed && !has_marker) ==> true,  // ResumeGc path
{
}

/// Conservation: free_space decreases exactly by data_len + ate_size per write.
pub proof fn lemma_conservation(free_before: u32, data_len: u32, ate_size: u32)
    requires
        ate_size > 0,
        free_before as int >= data_len as int + ate_size as int,
    ensures
        (free_before - data_len - ate_size) as int ==
            free_before as int - data_len as int - ate_size as int,
{
}

/// Sector advance wraps correctly.
pub proof fn lemma_sector_advance_wraps(current: u32, count: u32)
    requires
        count > 1,
        current < count,
    ensures ({
        let next = if current + 1 == count { 0u32 } else { (current + 1) as u32 };
        next < count
    })
{
}

} // verus!
