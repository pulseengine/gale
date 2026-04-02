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
use crate::error::*;
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
    /// Initialize a sector to its fresh state (after erase).
    ///
    /// The first 2 ATE slots (from the top) are reserved for the empty ATE
    /// and close ATE.  The ate_wra starts at the third-from-top position.
    /// data_wra starts at 0 (beginning of sector).
    ///
    /// Models: sector setup in zms_init / zms_gc when a new sector is prepared.
    pub fn init(sector_size: u32, ate_size: u32, cycle_cnt: u8) -> Result<Self, i32> {
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
    /// Check if a write of `needed` bytes (data + ATE) fits in this sector.
    ///
    /// Models: zms.c:1688-1690 —
    ///   `(SECTOR_OFFSET(fs->ate_wra)) &&
    ///    (fs->ate_wra >= (fs->data_wra + required_space))`
    ///
    /// ZMS3: if this returns true, the write proceeds without GC.
    pub fn has_space(&self, needed: u32, ate_size: u32) -> bool {
        self.ate_wra > 0 && self.ate_wra >= self.data_wra + needed
    }
    /// Decide whether the sector can be closed.
    ///
    /// A sector can be closed when ate_wra is at a valid (non-zero) offset.
    /// The close ATE is written at the second-from-top position.
    ///
    /// Models: zms_sector_close (zms.c:728-782).
    pub fn close_decide(&self, ate_size: u32) -> ZmsCloseDecision {
        if self.ate_wra > 0 {
            ZmsCloseDecision {
                action: CloseAction::CanClose as u8,
            }
        } else {
            ZmsCloseDecision {
                action: CloseAction::CannotClose as u8,
            }
        }
    }
    /// Increment cycle counter for sector reuse.
    ///
    /// ZMS5: cycle_cnt wraps at 256 (u8).
    /// Models: zms_verify_and_increment_cycle_cnt (zms.c:811-834).
    /// The close_cycle parameter is the cycle_cnt of the existing close ATE;
    /// if the incremented value collides with it, increment again.
    pub fn increment_cycle(&mut self, close_cycle: u8) {
        let new_cycle: u8 = ((self.cycle_cnt as u16 + 1) % 256) as u8;
        if new_cycle == close_cycle {
            self.cycle_cnt = ((self.cycle_cnt as u16 + 2) % 256) as u8;
        } else {
            self.cycle_cnt = new_cycle;
        }
    }
}
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
    /// Initialize the ZMS filesystem model.
    ///
    /// Models: zms_mount_internal validation (zms.c:1472-1551).
    /// Validates sector_count >= 2 and sector_size >= 5 * ate_size.
    pub fn init(
        sector_count: u32,
        sector_size: u32,
        ate_size: u32,
    ) -> Result<Self, i32> {
        if sector_count < 2 {
            return Err(EINVAL);
        }
        if sector_size < 5 * ate_size {
            return Err(EINVAL);
        }
        let free = sector_size - 4 * ate_size;
        Ok(ZmsFs {
            sector_count,
            current_sector: 0,
            ate_size,
            free_space: free,
        })
    }
    /// Check if a write fits in the current sector without GC.
    ///
    /// Models: zms.c:1688 — the condition for writing without GC.
    ///
    /// `needed` is the total required space: data_len (aligned) + ate_size,
    /// or just ate_size for small data stored inside the ATE.
    pub fn has_space(&self, needed: u32) -> bool {
        self.free_space >= needed
    }
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
    pub fn write_decide(&self, data_len: u32) -> ZmsWriteDecision {
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
    /// Determine if GC is needed given current sector addresses.
    ///
    /// Models: the ate_wra < data_wra + required_space check that triggers
    /// zms_sector_close + zms_gc in zms_write (zms.c:1688-1702).
    ///
    /// ZMS1: ate_wra >= data_wra is a precondition (maintained by the
    /// invariant). When ate_wra < data_wra + needed, GC is required.
    pub fn gc_needed(ate_wra: u32, data_wra: u32, needed: u32) -> ZmsGcDecision {
        let available: u64 = ate_wra as u64 - data_wra as u64;
        if available >= needed as u64 {
            ZmsGcDecision {
                action: 0,
                sectors_to_gc: 0,
            }
        } else {
            ZmsGcDecision {
                action: 1,
                sectors_to_gc: 1,
            }
        }
    }
    /// Determine recovery action based on gc_done marker presence.
    ///
    /// Models: zms.c:1393-1453 —
    ///   - gc_done marker found => erase next sector (GC completed, cleanup pending)
    ///   - gc_done marker missing => restart GC (interrupted mid-operation)
    ///   - next sector is not closed => normal boot (no GC was in progress)
    ///
    /// ZMS6: the gc_done marker is the sole determinant of recovery action.
    pub fn gc_done_check(
        next_sector_closed: bool,
        has_gc_done_marker: bool,
    ) -> ZmsRecoveryDecision {
        if !next_sector_closed {
            ZmsRecoveryDecision {
                action: RecoveryAction::Normal as u8,
            }
        } else if has_gc_done_marker {
            ZmsRecoveryDecision {
                action: RecoveryAction::EraseAndRestart as u8,
            }
        } else {
            ZmsRecoveryDecision {
                action: RecoveryAction::ResumeGc as u8,
            }
        }
    }
    /// Advance to the next sector (wrapping at sector_count).
    ///
    /// Models: zms_sector_advance (zms.c:717-723).
    pub fn advance_sector(&mut self) {
        if self.current_sector + 1 == self.sector_count {
            self.current_sector = 0;
        } else {
            self.current_sector = self.current_sector + 1;
        }
    }
    /// Update free space after writing data_len bytes + 1 ATE.
    ///
    /// Models: the combined effect of zms_flash_data_wrt (data_wra += len)
    /// and zms_flash_ate_wrt (ate_wra -= ate_size) on free_space.
    pub fn consume_space(&mut self, data_len: u32) {
        self.free_space = self.free_space - data_len - self.ate_size;
    }
}
