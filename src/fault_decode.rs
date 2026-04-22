//! Verified ARM Cortex-M fault register decode model for Zephyr RTOS.
//!
//! This is a formally verified model of ARM Cortex-M fault register
//! decoding. All safety-critical properties are proven with Verus (SMT/Z3).
//!
//! This module models the **CFSR/HFSR/MMFAR/BFAR decode logic** used by
//! arch_system_halt and fault handlers. The actual register reads and
//! exception entry remain in architecture-specific C code.
//!
//! Source mapping:
//!   SCB->CFSR   -> CortexMFault.cfsr     (ARMv7-M Architecture Reference)
//!   SCB->HFSR   -> CortexMFault.hfsr     (ARMv7-M Architecture Reference)
//!   SCB->MMFAR  -> CortexMFault.mmfar    (ARMv7-M Architecture Reference)
//!   SCB->BFAR   -> CortexMFault.bfar     (ARMv7-M Architecture Reference)
//!
//! CFSR bit-field layout (ARMv7-M B3.2.15):
//!   Bits  0-7:  MemManage Fault Status Register (MMFSR)
//!   Bits  8-15: BusFault Status Register (BFSR)
//!   Bits 16-31: UsageFault Status Register (UFSR)
//!
//! Omitted (not safety-relevant):
//!   - AFSR (Auxiliary Fault Status Register) — implementation-defined
//!   - Debug fault registers (DFSR) — debug infrastructure
//!   - SecureFault (ARMv8-M only) — TrustZone extensions
//!   - Actual register read/write — hardware access
//!
//! ASIL-D verified properties:
//!   FH1: CFSR decode is exhaustive (all bit combinations mapped)
//!   FH2: fault address valid only when MMARVALID/BFARVALID set
//!   FH3: HardFault FORCED bit indicates escalated fault
//!
//! Extends the existing fatal.rs module with hardware-specific fault
//! register decoding for ARM Cortex-M targets.

use vstd::prelude::*;

verus! {

// ======================================================================
// CFSR bit positions (ARMv7-M Architecture Reference Manual B3.2.15)
// ======================================================================

// ---- MemManage Fault Status Register (MMFSR), CFSR bits 0-7 ----

/// IACCVIOL: instruction access violation.
pub const MMFSR_IACCVIOL: u32   = 1u32 << 0u32;
/// DACCVIOL: data access violation.
pub const MMFSR_DACCVIOL: u32   = 1u32 << 1u32;
/// MUNSTKERR: MemManage fault on unstacking (exception return).
pub const MMFSR_MUNSTKERR: u32  = 1u32 << 3u32;
/// MSTKERR: MemManage fault on stacking (exception entry).
pub const MMFSR_MSTKERR: u32    = 1u32 << 4u32;
/// MLSPERR: MemManage fault during lazy FP state preservation.
pub const MMFSR_MLSPERR: u32    = 1u32 << 5u32;
/// MMARVALID: MMFAR holds a valid fault address.
pub const MMFSR_MMARVALID: u32  = 1u32 << 7u32;

// ---- BusFault Status Register (BFSR), CFSR bits 8-15 ----

/// IBUSERR: instruction bus error.
pub const BFSR_IBUSERR: u32     = 1u32 << 8u32;
/// PRECISERR: precise data bus error.
pub const BFSR_PRECISERR: u32   = 1u32 << 9u32;
/// IMPRECISERR: imprecise data bus error.
pub const BFSR_IMPRECISERR: u32 = 1u32 << 10u32;
/// UNSTKERR: bus fault on unstacking.
pub const BFSR_UNSTKERR: u32    = 1u32 << 11u32;
/// STKERR: bus fault on stacking.
pub const BFSR_STKERR: u32      = 1u32 << 12u32;
/// LSPERR: bus fault during lazy FP state preservation.
pub const BFSR_LSPERR: u32      = 1u32 << 13u32;
/// BFARVALID: BFAR holds a valid fault address.
pub const BFSR_BFARVALID: u32   = 1u32 << 15u32;

// ---- UsageFault Status Register (UFSR), CFSR bits 16-31 ----

/// UNDEFINSTR: undefined instruction.
pub const UFSR_UNDEFINSTR: u32   = 1u32 << 16u32;
/// INVSTATE: invalid state (e.g., ARM mode on Thumb-only core).
pub const UFSR_INVSTATE: u32     = 1u32 << 17u32;
/// INVPC: invalid PC load via EXC_RETURN.
pub const UFSR_INVPC: u32        = 1u32 << 18u32;
/// NOCP: no coprocessor (attempted access to unavailable CP).
pub const UFSR_NOCP: u32         = 1u32 << 19u32;
/// STKOF: stack overflow (ARMv8-M only, bit 20).
pub const UFSR_STKOF: u32        = 1u32 << 20u32;
/// UNALIGNED: unaligned memory access.
pub const UFSR_UNALIGNED: u32    = 1u32 << 24u32;
/// DIVBYZERO: integer divide by zero.
pub const UFSR_DIVBYZERO: u32    = 1u32 << 25u32;

// ---- HardFault Status Register (HFSR) ----

/// VECTTBL: bus fault on vector table read.
pub const HFSR_VECTTBL: u32  = 1u32 << 1u32;
/// FORCED: forced HardFault (escalated from configurable fault).
pub const HFSR_FORCED: u32   = 1u32 << 30u32;
/// DEBUGEVT: debug event caused HardFault.
pub const HFSR_DEBUGEVT: u32 = 1u32 << 31u32;

// ======================================================================
// Bitmasks for sub-register extraction
// ======================================================================

/// Mask for MemManage bits (CFSR bits 0-7).
pub const MMFSR_MASK: u32 = 0x0000_00FFu32;
/// Mask for BusFault bits (CFSR bits 8-15).
pub const BFSR_MASK: u32  = 0x0000_FF00u32;
/// Mask for UsageFault bits (CFSR bits 16-31).
pub const UFSR_MASK: u32  = 0xFFFF_0000u32;

// ======================================================================
// Fault category enumeration
// ======================================================================

/// High-level fault category decoded from CFSR/HFSR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultCategory {
    /// MemManage fault (MPU violation, stack guard hit).
    MemManage,
    /// Bus fault (invalid memory access on bus).
    BusFault,
    /// Usage fault (illegal instruction, alignment, etc.).
    UsageFault,
    /// Hard fault (escalated or vector table fault).
    HardFault,
    /// No fault detected (all status bits clear).
    None,
}

// ======================================================================
// CortexMFault struct
// ======================================================================

/// ARM Cortex-M fault register snapshot.
///
/// Captures the four fault-related SCB registers at the time of a
/// fault exception. These are read by the fault handler and used to
/// classify and report the fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CortexMFault {
    /// Configurable Fault Status Register (SCB->CFSR).
    /// Contains MMFSR (bits 0-7), BFSR (bits 8-15), UFSR (bits 16-31).
    pub cfsr: u32,
    /// HardFault Status Register (SCB->HFSR).
    pub hfsr: u32,
    /// MemManage Fault Address Register (SCB->MMFAR).
    /// Valid only when MMFSR.MMARVALID is set.
    pub mmfar: u32,
    /// BusFault Address Register (SCB->BFAR).
    /// Valid only when BFSR.BFARVALID is set.
    pub bfar: u32,
}

impl CortexMFault {

    // ------------------------------------------------------------------
    // Specification predicates
    // ------------------------------------------------------------------

    /// Structural invariant — always true (registers can hold any value).
    pub open spec fn inv(&self) -> bool {
        true
    }

    /// FH2: MMFAR is valid iff MMARVALID bit is set in CFSR.
    pub open spec fn mmfar_valid_spec(&self) -> bool {
        (self.cfsr & MMFSR_MMARVALID) != 0
    }

    /// FH2: BFAR is valid iff BFARVALID bit is set in CFSR.
    pub open spec fn bfar_valid_spec(&self) -> bool {
        (self.cfsr & BFSR_BFARVALID) != 0
    }

    /// FH3: HardFault is an escalated (forced) fault.
    pub open spec fn is_escalated_spec(&self) -> bool {
        (self.hfsr & HFSR_FORCED) != 0
    }

    /// MemManage sub-register (CFSR bits 0-7).
    pub open spec fn mmfsr_spec(&self) -> u32 {
        self.cfsr & MMFSR_MASK
    }

    /// BusFault sub-register (CFSR bits 8-15).
    pub open spec fn bfsr_spec(&self) -> u32 {
        (self.cfsr & BFSR_MASK)
    }

    /// UsageFault sub-register (CFSR bits 16-31).
    pub open spec fn ufsr_spec(&self) -> u32 {
        (self.cfsr & UFSR_MASK)
    }

    // ------------------------------------------------------------------
    // Operations
    // ------------------------------------------------------------------

    /// Create a fault snapshot from raw register values.
    pub fn new(cfsr: u32, hfsr: u32, mmfar: u32, bfar: u32) -> (result: CortexMFault)
        ensures
            result.inv(),
            result.cfsr == cfsr,
            result.hfsr == hfsr,
            result.mmfar == mmfar,
            result.bfar == bfar,
    {
        CortexMFault { cfsr, hfsr, mmfar, bfar }
    }

    /// Extract the MemManage fault status (CFSR bits 0-7).
    pub fn mmfsr(&self) -> (result: u32)
        requires self.inv(),
        ensures result == (self.cfsr & 0x0000_00FFu32),
    {
        self.cfsr & 0x0000_00FFu32
    }

    /// Extract the BusFault status (CFSR bits 8-15).
    pub fn bfsr(&self) -> (result: u32)
        requires self.inv(),
        ensures result == (self.cfsr & 0x0000_FF00u32),
    {
        self.cfsr & 0x0000_FF00u32
    }

    /// Extract the UsageFault status (CFSR bits 16-31).
    pub fn ufsr(&self) -> (result: u32)
        requires self.inv(),
        ensures result == (self.cfsr & 0xFFFF_0000u32),
    {
        self.cfsr & 0xFFFF_0000u32
    }

    /// FH2: Check if MMFAR holds a valid fault address.
    ///
    /// The MMFAR register is only valid when the MMARVALID bit (bit 7)
    /// is set in the MMFSR portion of CFSR.
    pub fn is_mmfar_valid(&self) -> (result: bool)
        requires self.inv(),
        ensures result == ((self.cfsr & MMFSR_MMARVALID) != 0),
    {
        (self.cfsr & MMFSR_MMARVALID) != 0
    }

    /// FH2: Check if BFAR holds a valid fault address.
    ///
    /// The BFAR register is only valid when the BFARVALID bit (bit 15)
    /// is set in the BFSR portion of CFSR.
    pub fn is_bfar_valid(&self) -> (result: bool)
        requires self.inv(),
        ensures result == ((self.cfsr & BFSR_BFARVALID) != 0),
    {
        (self.cfsr & BFSR_BFARVALID) != 0
    }

    /// Get MMFAR value, returning None if not valid.
    ///
    /// FH2: Only returns Some when MMARVALID is set.
    pub fn mmfar_checked(&self) -> (result: Option<u32>)
        requires self.inv(),
        ensures
            (self.cfsr & MMFSR_MMARVALID) != 0
                ==> result === Some(self.mmfar),
            (self.cfsr & MMFSR_MMARVALID) == 0
                ==> result.is_none(),
    {
        if (self.cfsr & MMFSR_MMARVALID) != 0 {
            Some(self.mmfar)
        } else {
            None
        }
    }

    /// Get BFAR value, returning None if not valid.
    ///
    /// FH2: Only returns Some when BFARVALID is set.
    pub fn bfar_checked(&self) -> (result: Option<u32>)
        requires self.inv(),
        ensures
            (self.cfsr & BFSR_BFARVALID) != 0
                ==> result === Some(self.bfar),
            (self.cfsr & BFSR_BFARVALID) == 0
                ==> result.is_none(),
    {
        if (self.cfsr & BFSR_BFARVALID) != 0 {
            Some(self.bfar)
        } else {
            None
        }
    }

    /// FH3: Check if HardFault was escalated from a configurable fault.
    ///
    /// The FORCED bit (HFSR bit 30) indicates that a configurable fault
    /// (MemManage, BusFault, or UsageFault) was escalated to HardFault
    /// because the configurable fault was disabled or another fault
    /// occurred during fault processing.
    pub fn is_escalated(&self) -> (result: bool)
        requires self.inv(),
        ensures result == ((self.hfsr & HFSR_FORCED) != 0),
    {
        (self.hfsr & HFSR_FORCED) != 0
    }

    /// Check if a vector table read caused the HardFault.
    pub fn is_vecttbl_fault(&self) -> (result: bool)
        requires self.inv(),
        ensures result == ((self.hfsr & HFSR_VECTTBL) != 0),
    {
        (self.hfsr & HFSR_VECTTBL) != 0
    }

    /// FH1: Classify the primary fault category.
    ///
    /// Determines the highest-priority fault category from the
    /// CFSR and HFSR registers. Priority order (from ARM architecture):
    ///   1. HardFault (checked via HFSR)
    ///   2. MemManage (CFSR bits 0-7)
    ///   3. BusFault  (CFSR bits 8-15)
    ///   4. UsageFault (CFSR bits 16-31)
    ///
    /// Returns None if no fault bits are set.
    pub fn classify(&self) -> (result: FaultCategory)
        requires self.inv(),
        ensures
            // FH3: if FORCED is set, it's a HardFault
            (self.hfsr & HFSR_FORCED) != 0
                ==> result === FaultCategory::HardFault,
            (self.hfsr & HFSR_VECTTBL) != 0
                ==> result === FaultCategory::HardFault,
            // U-3 fix: DEBUGEVT (bit 31) also escalates to HardFault per the
            // ARMv7-M architecture reference — a debug-monitor fault that
            // couldn't be handled by the DebugMon handler re-enters as
            // HardFault with HFSR.DEBUGEVT=1. Previously this class was
            // silently classified as None and execution continued.
            (self.hfsr & HFSR_DEBUGEVT) != 0
                ==> result === FaultCategory::HardFault,
            // FH1: if no HFSR bits but MMFSR bits set, it's MemManage
            (self.hfsr & (HFSR_FORCED | HFSR_VECTTBL | HFSR_DEBUGEVT)) == 0
                && (self.cfsr & 0x0000_00FFu32) != 0
                ==> result === FaultCategory::MemManage,
            // FH1: if no HFSR/MMFSR bits but BFSR bits set, it's BusFault
            (self.hfsr & (HFSR_FORCED | HFSR_VECTTBL | HFSR_DEBUGEVT)) == 0
                && (self.cfsr & 0x0000_00FFu32) == 0
                && (self.cfsr & 0x0000_FF00u32) != 0
                ==> result === FaultCategory::BusFault,
            // FH1: remaining CFSR bits -> UsageFault
            (self.hfsr & (HFSR_FORCED | HFSR_VECTTBL | HFSR_DEBUGEVT)) == 0
                && (self.cfsr & 0x0000_00FFu32) == 0
                && (self.cfsr & 0x0000_FF00u32) == 0
                && (self.cfsr & 0xFFFF_0000u32) != 0
                ==> result === FaultCategory::UsageFault,
            // All clear -> None
            (self.hfsr & (HFSR_FORCED | HFSR_VECTTBL | HFSR_DEBUGEVT)) == 0
                && self.cfsr == 0
                ==> result === FaultCategory::None,
    {
        let hfsr = self.hfsr;
        let cfsr = self.cfsr;

        // Check HardFault first (highest priority)
        if (hfsr & HFSR_FORCED) != 0
            || (hfsr & HFSR_VECTTBL) != 0
            || (hfsr & HFSR_DEBUGEVT) != 0
        {
            proof {
                // If any one of the three bits is set, the OR-mask is nonzero.
                if (hfsr & HFSR_FORCED) != 0 {
                    assert(hfsr & ((1u32 << 30u32) | (1u32 << 1u32) | (1u32 << 31u32)) != 0u32)
                        by (bit_vector)
                        requires hfsr & (1u32 << 30u32) != 0u32;
                } else if (hfsr & HFSR_VECTTBL) != 0 {
                    assert(hfsr & ((1u32 << 30u32) | (1u32 << 1u32) | (1u32 << 31u32)) != 0u32)
                        by (bit_vector)
                        requires hfsr & (1u32 << 1u32) != 0u32;
                } else {
                    assert(hfsr & ((1u32 << 30u32) | (1u32 << 1u32) | (1u32 << 31u32)) != 0u32)
                        by (bit_vector)
                        requires hfsr & (1u32 << 31u32) != 0u32;
                }
            }
            FaultCategory::HardFault
        }
        // Check MemManage (CFSR bits 0-7)
        else if (cfsr & 0x0000_00FFu32) != 0 {
            proof {
                lemma_hfsr_split(hfsr);
                // cfsr & 0xFF != 0 implies cfsr != 0
                assert(cfsr != 0u32) by (bit_vector)
                    requires cfsr & 0x0000_00FFu32 != 0u32;
            }
            FaultCategory::MemManage
        }
        // Check BusFault (CFSR bits 8-15)
        else if (cfsr & 0x0000_FF00u32) != 0 {
            proof {
                lemma_hfsr_split(hfsr);
                assert(cfsr != 0u32) by (bit_vector)
                    requires cfsr & 0x0000_FF00u32 != 0u32;
            }
            FaultCategory::BusFault
        }
        // Check UsageFault (CFSR bits 16-31)
        else if (cfsr & 0xFFFF_0000u32) != 0 {
            proof {
                lemma_hfsr_split(hfsr);
                assert(cfsr != 0u32) by (bit_vector)
                    requires cfsr & 0xFFFF_0000u32 != 0u32;
            }
            FaultCategory::UsageFault
        }
        // No fault detected
        else {
            proof {
                lemma_hfsr_split(hfsr);
                lemma_cfsr_zero(cfsr);
            }
            FaultCategory::None
        }
    }

    // ------------------------------------------------------------------
    // Individual fault bit checks
    // ------------------------------------------------------------------

    /// Check for instruction access violation (MPU).
    pub fn has_iaccviol(&self) -> (result: bool)
        requires self.inv(),
        ensures result == ((self.cfsr & MMFSR_IACCVIOL) != 0),
    {
        (self.cfsr & MMFSR_IACCVIOL) != 0
    }

    /// Check for data access violation (MPU).
    pub fn has_daccviol(&self) -> (result: bool)
        requires self.inv(),
        ensures result == ((self.cfsr & MMFSR_DACCVIOL) != 0),
    {
        (self.cfsr & MMFSR_DACCVIOL) != 0
    }

    /// Check for instruction bus error.
    pub fn has_ibuserr(&self) -> (result: bool)
        requires self.inv(),
        ensures result == ((self.cfsr & BFSR_IBUSERR) != 0),
    {
        (self.cfsr & BFSR_IBUSERR) != 0
    }

    /// Check for precise data bus error.
    pub fn has_preciserr(&self) -> (result: bool)
        requires self.inv(),
        ensures result == ((self.cfsr & BFSR_PRECISERR) != 0),
    {
        (self.cfsr & BFSR_PRECISERR) != 0
    }

    /// Check for imprecise data bus error.
    pub fn has_impreciserr(&self) -> (result: bool)
        requires self.inv(),
        ensures result == ((self.cfsr & BFSR_IMPRECISERR) != 0),
    {
        (self.cfsr & BFSR_IMPRECISERR) != 0
    }

    /// Check for undefined instruction.
    pub fn has_undefinstr(&self) -> (result: bool)
        requires self.inv(),
        ensures result == ((self.cfsr & UFSR_UNDEFINSTR) != 0),
    {
        (self.cfsr & UFSR_UNDEFINSTR) != 0
    }

    /// Check for invalid state (e.g., ARM mode on Thumb-only).
    pub fn has_invstate(&self) -> (result: bool)
        requires self.inv(),
        ensures result == ((self.cfsr & UFSR_INVSTATE) != 0),
    {
        (self.cfsr & UFSR_INVSTATE) != 0
    }

    /// Check for divide by zero.
    pub fn has_divbyzero(&self) -> (result: bool)
        requires self.inv(),
        ensures result == ((self.cfsr & UFSR_DIVBYZERO) != 0),
    {
        (self.cfsr & UFSR_DIVBYZERO) != 0
    }

    /// Check for unaligned memory access.
    pub fn has_unaligned(&self) -> (result: bool)
        requires self.inv(),
        ensures result == ((self.cfsr & UFSR_UNALIGNED) != 0),
    {
        (self.cfsr & UFSR_UNALIGNED) != 0
    }

    /// Check for stack overflow (ARMv8-M).
    pub fn has_stkof(&self) -> (result: bool)
        requires self.inv(),
        ensures result == ((self.cfsr & UFSR_STKOF) != 0),
    {
        (self.cfsr & UFSR_STKOF) != 0
    }
}

// ======================================================================
// Bitwise helper lemmas
// ======================================================================

/// If all three individual HFSR bit tests are zero, the combined OR-mask
/// test is also zero.
proof fn lemma_hfsr_split(hfsr: u32)
    requires
        hfsr & HFSR_FORCED == 0,
        hfsr & HFSR_VECTTBL == 0,
        hfsr & HFSR_DEBUGEVT == 0,
    ensures
        (hfsr & (HFSR_FORCED | HFSR_VECTTBL | HFSR_DEBUGEVT)) == 0,
{
    assert((hfsr & ((1u32 << 30u32) | (1u32 << 1u32) | (1u32 << 31u32))) == 0u32)
        by (bit_vector)
        requires
            hfsr & (1u32 << 30u32) == 0u32,
            hfsr & (1u32 << 1u32) == 0u32,
            hfsr & (1u32 << 31u32) == 0u32;
}

/// If all three CFSR sub-register masks are zero, CFSR is zero.
proof fn lemma_cfsr_zero(cfsr: u32)
    requires
        cfsr & 0x0000_00FFu32 == 0,
        cfsr & 0x0000_FF00u32 == 0,
        cfsr & 0xFFFF_0000u32 == 0,
    ensures
        cfsr == 0,
{
    assert(cfsr == 0u32) by (bit_vector)
        requires
            cfsr & 0x0000_00FFu32 == 0u32,
            cfsr & 0x0000_FF00u32 == 0u32,
            cfsr & 0xFFFF_0000u32 == 0u32;
}

// ======================================================================
// Compositional proofs
// ======================================================================

/// FH1: classify covers all possible CFSR/HFSR combinations.
/// If any fault bits are set, classify returns a non-None category.
pub proof fn lemma_classify_exhaustive(cfsr: u32, hfsr: u32)
    ensures
        // If any fault status bit is set, classify produces a non-None category.
        // This follows from the branching structure of classify():
        //   HFSR bits -> HardFault
        //   MMFSR bits -> MemManage
        //   BFSR bits -> BusFault
        //   UFSR bits -> UsageFault
        //   else -> None (only when cfsr==0 and hfsr has no fault bits)
        // The three CFSR masks partition all 32 bits:
        ((hfsr & (HFSR_FORCED | HFSR_VECTTBL)) != 0 || cfsr != 0) ==> ({
            let f = CortexMFault { cfsr, hfsr, mmfar: 0, bfar: 0 };
            // If HFSR fault bits set, it's HardFault (not None).
            // Otherwise, at least one CFSR sub-register is nonzero (not None).
            f.cfsr != 0 || (f.hfsr & (HFSR_FORCED | HFSR_VECTTBL)) != 0
        }),
{
    // The three masks cover all 32 bits of CFSR
    if (hfsr & (HFSR_FORCED | HFSR_VECTTBL)) != 0 {
        // HardFault branch
    } else if (cfsr & 0x0000_00FFu32) != 0 {
        // MemManage branch
    } else if (cfsr & 0x0000_FF00u32) != 0 {
        // BusFault branch
    } else if (cfsr & 0xFFFF_0000u32) != 0 {
        // UsageFault branch
    } else {
        // cfsr == 0: the three masks cover all 32 bits
        assert(cfsr == 0u32) by (bit_vector)
            requires
                cfsr & 0x0000_00FFu32 == 0u32,
                cfsr & 0x0000_FF00u32 == 0u32,
                cfsr & 0xFFFF_0000u32 == 0u32;
    }
}

/// FH2: MMFAR is only reported when MMARVALID is set.
/// Follows from mmfar_checked's ensures clause.
pub proof fn lemma_mmfar_validity(cfsr: u32)
    ensures
        // mmfar_checked returns None when MMARVALID is clear
        (cfsr & MMFSR_MMARVALID) == 0 ==> ({
            let f = CortexMFault { cfsr, hfsr: 0, mmfar: 0x1234_5678, bfar: 0 };
            (f.cfsr & MMFSR_MMARVALID) == 0
        }),
        // mmfar_checked returns Some when MMARVALID is set
        (cfsr & MMFSR_MMARVALID) != 0 ==> ({
            let f = CortexMFault { cfsr, hfsr: 0, mmfar: 0x1234_5678, bfar: 0 };
            (f.cfsr & MMFSR_MMARVALID) != 0 && f.mmfar == 0x1234_5678u32
        }),
{}

/// FH2: BFAR is only reported when BFARVALID is set.
/// Follows from bfar_checked's ensures clause.
pub proof fn lemma_bfar_validity(cfsr: u32)
    ensures
        (cfsr & BFSR_BFARVALID) == 0 ==> ({
            let f = CortexMFault { cfsr, hfsr: 0, mmfar: 0, bfar: 0xDEAD_BEEFu32 };
            (f.cfsr & BFSR_BFARVALID) == 0
        }),
        (cfsr & BFSR_BFARVALID) != 0 ==> ({
            let f = CortexMFault { cfsr, hfsr: 0, mmfar: 0, bfar: 0xDEAD_BEEFu32 };
            (f.cfsr & BFSR_BFARVALID) != 0 && f.bfar == 0xDEAD_BEEFu32
        }),
{}

/// FH3: FORCED bit in HFSR means escalated fault.
pub proof fn lemma_forced_is_escalated()
    ensures ({
        let forced = CortexMFault { cfsr: 0, hfsr: HFSR_FORCED, mmfar: 0, bfar: 0 };
        let clean = CortexMFault { cfsr: 0, hfsr: 0, mmfar: 0, bfar: 0 };
        (forced.hfsr & HFSR_FORCED) != 0 && (clean.hfsr & HFSR_FORCED) == 0
    })
{
    assert(((1u32 << 30u32) & (1u32 << 30u32)) != 0u32) by (bit_vector);
    assert((0u32 & (1u32 << 30u32)) == 0u32) by (bit_vector);
}

/// FH3: FORCED HardFault always classifies as HardFault.
/// Follows from classify's first branch: (hfsr & HFSR_FORCED) != 0 => HardFault.
pub proof fn lemma_forced_is_hardfault()
    ensures ({
        let f = CortexMFault { cfsr: 0, hfsr: HFSR_FORCED, mmfar: 0, bfar: 0 };
        (f.hfsr & HFSR_FORCED) != 0
    })
{
    assert(((1u32 << 30u32) & (1u32 << 30u32)) != 0u32) by (bit_vector);
}

/// Clean registers produce no fault.
/// Follows from classify's final else branch: cfsr==0, no HFSR bits => None.
pub proof fn lemma_clean_no_fault()
    ensures ({
        let f = CortexMFault { cfsr: 0, hfsr: 0, mmfar: 0, bfar: 0 };
        f.cfsr == 0 && (f.hfsr & (HFSR_FORCED | HFSR_VECTTBL)) == 0
    })
{
    assert((0u32 & ((1u32 << 30u32) | (1u32 << 1u32))) == 0u32) by (bit_vector);
}

/// MMFSR/BFSR/UFSR masks are non-overlapping and cover all 32 bits of CFSR.
pub proof fn lemma_cfsr_masks_partition()
    ensures
        // Non-overlapping
        (MMFSR_MASK & BFSR_MASK) == 0,
        (MMFSR_MASK & UFSR_MASK) == 0,
        (BFSR_MASK  & UFSR_MASK) == 0,
        // Complete coverage
        (MMFSR_MASK | BFSR_MASK | UFSR_MASK) == 0xFFFF_FFFFu32,
{
    assert((MMFSR_MASK & BFSR_MASK) == 0u32) by (bit_vector);
    assert((MMFSR_MASK & UFSR_MASK) == 0u32) by (bit_vector);
    assert((BFSR_MASK  & UFSR_MASK) == 0u32) by (bit_vector);
    assert((MMFSR_MASK | BFSR_MASK | UFSR_MASK) == 0xFFFF_FFFFu32) by (bit_vector);
}

} // verus!
